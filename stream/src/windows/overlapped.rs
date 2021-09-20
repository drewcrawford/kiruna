/*! Windows overlapped IO\

 This module has code common to [super::read] and [super::write] for overlapped I/O.

Generally, a specific operation provides a unique [PayloadTrait] implementation which is then
handed to the [Parent] type.
 */

use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker, Context};
use std::mem::MaybeUninit;
use winbind::Windows::Win32::Foundation::HANDLE;
use crate::windows::threadpool::{TaskHandle, add_task, remove_task};
use winbind::Windows::Win32::System::SystemServices::OVERLAPPED;
use std::future::Future;
use std::pin::Pin;

enum State<Payload,Ok,Failure> {
    NotPolled(Payload),
    Polled(Arc<Mutex<Shared<Ok,Failure>>>)
}
enum PolledResult<Result> {
    ///in progress or completed
    Poll(Poll<Result>),
    ///was returned to system already, bug in client
    Gone
}
impl<Result> PolledResult<Result> {
    ///Takes the Poll case if there is a Ready result, otherwise None
    fn maybe_take(&mut self) -> Option<Result> {
        match self {
            PolledResult::Poll(p) => {
                if p.is_ready() {
                    //take
                    let mut temp = PolledResult::Gone;
                    std::mem::swap(self, &mut temp);
                    let r = match temp {
                        PolledResult::Poll(Poll::Ready(value)) => {
                            value
                        }
                        _ => unreachable!() //checked this above
                    };
                    Some(r)
                }
                else {
                    None
                }
            }
            PolledResult::Gone => {
                panic!("Polled too many times!")
            }
        }
    }
}

///Data shared by both [Parent] and [Child]
struct Shared<Ok,Failure> {
    waker: Waker,
    result: PolledResult<Result<Ok,Failure>>
}

impl<Ok,Failure> Shared<Ok,Failure> {
    fn new(waker: Waker) -> Self {
        Self {
            waker,
            result: PolledResult::Poll(Poll::Pending)
        }
    }
}

///This is the version that is used inside the op (in the threadpool)
struct Child<Payload,Ok,Failure> {
    payload: Payload,
    //allocate space for overlapped, apparently not kept by OS
    overlapped: MaybeUninit<OVERLAPPED>,
    handle: HANDLE,
    shared: Arc<Mutex<Shared<Ok,Failure>>>,
    task: TaskHandle,
}
///Top-level future type for overlapped I/O operation(s).
///
/// One instance can handle a series of sequential reads/writes.
///
/// This type implements [Future] and can be awaited directly.
pub struct Parent<Payload,Ok,Failure> {
    ///file handle
    handle: HANDLE,
    ///current state
    state: State<Payload,Ok,Failure>,
}
impl<Payload,Ok,Failure> Parent<Payload,Ok,Failure> {
    pub fn new(handle: HANDLE, payload: Payload) -> Self {
        Self {
            handle,
            state: State::NotPolled(payload)
        }
    }
}


impl<Payload: PayloadTrait + Send + 'static> Future for Parent<Payload,Payload::Ok,Payload::Failure> where Payload::Ok: Send + 'static, Payload::Failure: Send + 'static{
    type Output = Result<Payload::Ok,Payload::Failure>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe {
            let s = self.get_unchecked_mut();
            match &mut s.state {
                State::NotPolled(_) => {
                    let move_handle = s.handle;
                    let move_shared = Arc::new(Mutex::new(Shared::new(cx.waker().clone())));

                    let mut swap_state = State::Polled(move_shared.clone());
                    std::mem::swap(&mut s.state, &mut swap_state);
                    let move_payload = match swap_state {
                        State::NotPolled(payload) => {payload}
                        State::Polled(_) => {unreachable!()}
                    };
                    add_task(move |task| {
                        //we need to box this, as it must survive even if the future is dropped.
                        let child_task = Box::new(Child::new(move_handle, move_shared,task, move_payload));
                        child_task.begin_op();
                    });
                    Poll::Pending

                }
                State::Polled(shared) => {
                    let shared = &mut shared.lock().unwrap();
                    match shared.result.maybe_take() {
                        None => {
                            shared.waker = cx.waker().clone();
                            Poll::Pending
                        }
                        Some(result) => {
                            Poll::Ready(result)
                        }
                    }

                }
            }
        }

    }
}

///Implement this trait to implement a specific overlapped I/O operation.
#[allow(non_snake_case)]
pub trait PayloadTrait {
    ///Success type
    type Ok;
    ///Err type
    type Failure;
    ///Implement this to begin an overlapped read
    /// * `self` The pinned payload
    /// * `handle`: The file handle for reading/writing
    /// * `overlapped`: A reference to the overlapped structure.  This is memory-managed for you
    /// * `hEvent`: You *MUST* set [OVERLAPPED.hEvent] to this value.
    /// * `completion`: Pass this completion value to your AIO call.  kiruna will then invoke `resume_op` from that handler.  This handler
    /// deals with various things like memory managing the arguments.
    ///
    /// Return: If you return Poll::Ready you will not be called again (and do not need to use completion).  Otherwise you should have an outstanding IO call with the `completion` argument at return-time.

    fn begin_op(self: Pin<&mut Self>, handle: HANDLE, overlapped: Pin<&mut MaybeUninit<OVERLAPPED>>,hEvent: HANDLE, completion: unsafe extern "system" fn(u32, u32, *mut OVERLAPPED)) -> Poll<Result<Self::Ok,Self::Failure>>;
    ///Resume an overlapped read.  For example, you may handle the received read or start a new read.
    /// * `self`: The pinned payload
    /// * `error_code`: The argument to the overlapped completion handler
    /// * `bytes_transferred`: The argument to the overlapped completion handler
    /// * `overlapped`: Pinned overlapped structure, this is memory-managed for you
    /// * `hEvent`: You *MUST* set [OVERLAPPED.hEvent] to this value.
    /// * `completion`: Pass this completion value to any AIO call you make in this function.  kiruna will then invoke `resume_op` again.  This handler
    /// deals with various things like memory managing the arguments.
    ///
    /// Return: If you return [Poll::Ready] you will not be called again (and do not need to use completion).   Otherwise you should have an outstanding IO call with the `completion` argument at return-time.
    fn resume_op(self: Pin<&mut Self>, error_code: u32, bytes_transferred: u32, handle: HANDLE,overlapped: Pin<&mut OVERLAPPED>, hEvent: HANDLE, completion:unsafe extern "system" fn(u32, u32, *mut OVERLAPPED)) -> Poll<Result<Self::Ok, Self::Failure>>;
}

extern "system" fn completion<Payload: PayloadTrait>(error_code: u32, bytes_transferred: u32, overlapped: *mut OVERLAPPED) {
    // println!("completion error_code {} bytes_transferred {}",error_code, bytes_transferred);
    unsafe {
        let overlapped = &*overlapped;
        let child_raw = overlapped.hEvent.0;
        let mut child: Box<Child<Payload,Payload::Ok,Payload::Failure>> = Box::from_raw(child_raw as *mut Child<Payload,Payload::Ok,Payload::Failure>);
        //structurally pin child for the benefit of the payload call
        let pinned = Pin::new_unchecked(&mut child.payload);
        let copy_hevent = overlapped.hEvent;
        let overlapped = Pin::new_unchecked(std::mem::transmute(&mut child.overlapped));
        let result = pinned.resume_op(error_code, bytes_transferred, child.handle,overlapped,copy_hevent, completion::<Payload>);
        match result {
            Poll::Ready(r) => {
                //drop box
                child.ready(r);
            }
            Poll::Pending => {
                std::mem::forget(child); //go around again
            }
        }
    }
}

impl<Payload: PayloadTrait> Child<Payload,Payload::Ok,Payload::Failure> {
    fn new(handle: HANDLE,shared: Arc<Mutex<Shared<Payload::Ok,Payload::Failure>>>,task: TaskHandle, payload: Payload) -> Self {
        Self {
            payload,
            handle,
            shared,
            overlapped: MaybeUninit::uninit(),
            task,
        }
    }
    ///Begins an op.  Note that self here takes a box arg, it is technically pinned as well, although the
    ///only valid way to pin it is on the heap, so I think using Box directly makes the most sense here
    fn begin_op(self: Box<Self>) {
        //convert to raw ptr
        let raw_ptr = Box::into_raw(self);
        let as_mut = unsafe {&mut *raw_ptr};
        //structurally pin some fields for benefit of the payload, this avoids exposing them to our memory management strategy
        let pinned = unsafe{ Pin::new_unchecked(&mut as_mut.payload)};
        let overlapped = unsafe { Pin::new_unchecked(&mut as_mut.overlapped)};
        let result = pinned.begin_op(as_mut.handle, overlapped, HANDLE(raw_ptr as isize), completion::<Payload>);
        match result {
            Poll::Ready(r) => {
                 as_mut.ready(r);
                //drop box
                let _drop = unsafe{ Box::from_raw(raw_ptr)};
            }
            Poll::Pending => {
                //leak box, cleaned up in resume_op
            }
        }
    }
    ///Result is ready and op is complete
    fn ready(&mut self, ready: Result<Payload::Ok,Payload::Failure>) {
        let mut lock = self.shared.lock().unwrap();
        lock.result = PolledResult::Poll(Poll::Ready(ready));
        lock.waker.wake_by_ref();
        remove_task(self.task);
    }
}
