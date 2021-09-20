use crate::windows::OSError;
use std::os::windows::io::{IntoRawHandle, RawHandle};
use std::mem::{MaybeUninit};
use std::ffi::c_void;
use winbind::Windows::Win32::System::SystemServices::{OVERLAPPED,OVERLAPPED_0,OVERLAPPED_0_0};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use winbind::Windows::Win32::System::Diagnostics::Debug::GetLastError;
use winbind::Windows::Win32::Foundation::HANDLE;
use crate::windows::threadpool::{add_task, remove_task, TaskHandle};
use std::marker::PhantomPinned;
use std::sync::{Mutex, Arc};
use winbind::Windows::Win32::System::Diagnostics::Debug::WIN32_ERROR;


///Reads from a file descriptor
pub struct Read {
    fd: RawHandle
}

#[derive(Debug)]
pub struct ReadBuffer(Vec<u8>);

impl ReadBuffer {
    pub fn into_contiguous(self) -> ContiguousReadBuffer {
        ContiguousReadBuffer(self.0)
    }
}

///Actually, both buffers are contiguous on Windows, but we support the same API for compatibiltiy.
pub struct ContiguousReadBuffer(Vec<u8>);
impl ContiguousReadBuffer {
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}



pub struct OSReadOptions;

enum State {
    NotPolled,
    Polled(Arc<Mutex<ReadOpShared>>)
}
enum PolledResult {
    ///in progress or completed
    Poll(Poll<Result<ReadBuffer, (OSError, ReadBuffer)>>),
    ///was returned to system already, bug in client
    Gone
}
impl PolledResult {
    ///Takes the Poll case if there is a Ready result, otherwise None
    fn maybe_take(&mut self) -> Option<Result<ReadBuffer, (OSError, ReadBuffer)>> {
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

///Data shared by both [ReadOpParent] and [ReadOpChild]
struct ReadOpShared {
    waker: Waker,
    result: PolledResult
}
impl ReadOpShared {
    fn new(waker: Waker) -> Self {
        Self {
            waker,
            result: PolledResult::Poll(Poll::Pending)
        }
    }
}

///This is the version that is used inside the read op (in the threadpool)
struct ReadOpChild {
    buffer: ReadBuffer,
    //inner buffer ptr, overlapped are passed to OS.  So we pin this struct.
    _pinned: PhantomPinned,
    //allocate space for overlapped, apparently not kept by OS
    overlapped: MaybeUninit<OVERLAPPED>,
    handle: HANDLE,
    shared: Arc<Mutex<ReadOpShared>>,
    task: TaskHandle,
}

///This is the version that is awaited
struct ReadOpParent {
    handle: HANDLE,
    state: State,
}
impl Future for ReadOpParent {
    type Output = Result<ReadBuffer,(OSError,ReadBuffer)>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
       unsafe {
           let s = self.get_unchecked_mut();
           match &mut s.state {
               State::NotPolled => {
                   let move_handle = s.handle;
                   let move_shared = Arc::new(Mutex::new(ReadOpShared::new(cx.waker().clone())));
                   s.state = State::Polled(move_shared.clone());

                   add_task(move |task| {
                       //we need to box this, as it must survive even if the future is dropped.
                       let child_task = Box::new(ReadOpChild::new(move_handle, move_shared,task));
                       child_task.begin_read();
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
impl ReadOpParent {
    fn new(handle: HANDLE) -> Self {
        Self {
            state: State::NotPolled,
            handle,
        }
    }
}

const READ_SIZE: usize = 255; //not sure what value to pick here

impl ReadOpChild {
    fn new(handle: HANDLE,shared: Arc<Mutex<ReadOpShared>>,task: TaskHandle) -> Self {
        Self {
            buffer: ReadBuffer(Vec::with_capacity(READ_SIZE)),
            handle,
            _pinned: PhantomPinned,
            shared,
            overlapped: MaybeUninit::uninit(),
            task,
        }
    }
    ///Mark the result as failed
    unsafe fn mark_failed(self,why: OSError) {
        //unsafely move vec into the shared area
        //this should be fine because of fn safety guarantee
        self.shared.lock().unwrap().result = PolledResult::Poll(Poll::Ready(Err((why,self.buffer))));
        remove_task(self.task);
    }
    ///Begins a read op.  Note that self here takes a box arg, it is technically pinned as well, although the
    ///only valid way to pin it is on the heap, so I think using Box directly makes the most sense here
    fn begin_read(self: Box<Self>) {
        //first, leak this type so that the raw ptr can be passed into fn
        let raw_ptr = Box::into_raw(self);
        //now, get a ref for use again in the function.  Have to be careful
        //not to leave this lying around.
        //Technically, this creates an overlapping exclusive pointer
        let as_mut = unsafe {&mut *raw_ptr};

        as_mut.buffer.0.reserve(READ_SIZE);

        unsafe {
            *as_mut.overlapped.assume_init_mut() = OVERLAPPED {
                //When the request is issued, the system sets this member
                Internal: MaybeUninit::uninit().assume_init(),
                //The system sets this member
                InternalHigh: MaybeUninit::uninit().assume_init(),
                Anonymous: OVERLAPPED_0 {
                    Anonymous: OVERLAPPED_0_0 {
                        /*The Offset and OffsetHigh members together represent a 64-bit file position. It is a byte offset from the start of the file or file-like device,
                        and it is specified by the user; the system will not modify these values. */
                        Offset:as_mut.buffer.0.len() as u32,
                        OffsetHigh: 0,
                    },
                },
                //The ReadFileEx function ignores the OVERLAPPED structure's hEvent member
                //we can (ab)use it to store a pointer to our child
                //note that this incidentally involves creating overlapping exclusive pointers to the struct, one is here and the other
                //is an argument to read
                hEvent: HANDLE(raw_ptr as isize),
            };
            use winbind::Windows::Win32::Storage::FileSystem::ReadFileEx;
            //note that on windows, this will return on the current thread, therefore we don't have to deal with Send etc
            let next_write = as_mut.buffer.0.as_mut_ptr().add(as_mut.buffer.0.len());

            let result = ReadFileEx(as_mut.handle, next_write as *mut c_void, READ_SIZE as u32, as_mut.overlapped.assume_init_mut(), Some(completion));
            if result.0 != 0 {
                //everything is fine
            }
            else {
                //get back to an owned pointer
                let owned = Box::from_raw(raw_ptr);
                owned.mark_failed(OSError(GetLastError()));
            }
        }
    }
}
extern "system" fn completion(error_code: u32, bytes_transferred: u32, overlapped: *mut OVERLAPPED) {
    // println!("completion error_code {} bytes_transferred {}",error_code, bytes_transferred);
    unsafe {
        let overlapped = &*overlapped;
        let child_raw = overlapped.hEvent.0;
        let mut child = Box::from_raw(child_raw as *mut ReadOpChild);
        if error_code != 0 {
            child.mark_failed(OSError(WIN32_ERROR(error_code)));
        }
        else {
            let len = child.buffer.0.len();
            child.buffer.0.set_len(len + bytes_transferred as usize);
            //begin a new read
            child.begin_read();
        }

    }
}
impl Read {
    pub fn new<H: IntoRawHandle>(handle: H) -> Self {
        Self {
            fd: handle.into_raw_handle()
        }
    }

    ///Reads the entire fd into memory
    pub async fn all<'a, O: Into<OSReadOptions>>(&self, _os_read_options: O) -> Result<ReadBuffer,(OSError,ReadBuffer)> {
        let as_handle =  winbind::Windows::Win32::Foundation::HANDLE(self.fd as isize);
            let fut = ReadOpParent::new(as_handle);
            let result = fut.await;
            use winbind::Windows::Win32::System::Diagnostics::Debug::ERROR_BROKEN_PIPE;
            match result {
                //this is really a success message in this context, basically EOF
                Err((OSError(ERROR_BROKEN_PIPE),new_buffer)) => {
                    return Ok(new_buffer);
                }
                other => {
                    return other;
                }
            }

    }
}

#[cfg(test)] mod tests {
    use std::process::{Command, Stdio};
    use crate::windows::read::{Read, OSReadOptions};

    #[test] fn read_process() {
        let c = Command::new("systeminfo").stdout(Stdio::piped()).spawn().unwrap();
        let read = Read::new(c.stdout.unwrap());
        let future = read.all(OSReadOptions);
        let result = kiruna::test::test_await(future, std::time::Duration::from_secs(10)).unwrap();
        println!("result length {:?}",result.0.len());
        println!("result {:?}",result.0);
    }
}