use std::future::Future;
use std::fmt::Debug;
use std::pin::Pin;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

///Transforms a future with a result, into a future that panics.  This is used to erase the "error" type
/// for passing to an executor.
pub async fn panic<F: Future<Output=Result<O, E>>, O, E: Debug>(future: F) -> O {
    let r = future.await;
    if r.is_ok() {
        r.unwrap()
    } else {
        panic!("kiruna::future::panic {:?}", r.err().unwrap());
    }
}

#[derive(Debug)]
pub struct PanicFuture<F: Future + ?Sized>(F);

impl<O, E: Debug, F: Future<Output=Result<O, E>>> Future for PanicFuture<F> {
    type Output = O;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        //I'm not confident if this is totally safe.  But I don't see how we're moving the value, just passing along
        //a value that is already pinned, so I *think* that's fine?
        let self_mut = unsafe { self.get_unchecked_mut() };
        let pinned_field = unsafe { Pin::new_unchecked(&mut self_mut.0) };
        if let Poll::Ready(value) = pinned_field.poll(cx) {
            match value {
                Ok(ok) => { Poll::Ready(ok) }
                Err(err) => {
                    panic!("kiruna::future::panic {:?}", err)
                }
            }
        } else {
            Poll::Pending
        }
    }
}

pub trait KirunaPanic: Future {
    type O;
    fn panic(self) -> PanicFuture<Self>;
}

impl<O, E, F> KirunaPanic for F where F: Future<Output=Result<O, E>> {
    type O = O;

    fn panic(self) -> PanicFuture<Self> {
        PanicFuture(self)
    }
}

pub mod prelude {
    pub use super::KirunaPanic;
}

/**
Maps a future by performing some operation.
 */
pub struct Map<Fut, Operation> {
    future: Fut,
    operation: Option<Operation>,
}
impl<Fut,Operation> Map<Fut,Operation> {
    #[inline] pub fn new(future: Fut, operation: Operation) -> Self {
        Self{future,operation: Some(operation)}
    }
}

impl<Fut,Operation,Output> Future for Map<Fut,Operation> where Fut: Future, Operation: FnOnce(Fut::Output) -> Output, Operation: Unpin {
    type Output = Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let project = unsafe{self.as_mut().map_unchecked_mut(|s| &mut s.future)};
        let result = project.poll(cx);

        match result {
            Poll::Pending => Poll::Pending,
            Poll::Ready(o) => {
                let f: &mut Option<Operation> = unsafe{self.as_mut().map_unchecked_mut(|s| &mut s.operation)}.get_mut();

                let f: Operation = f.take().unwrap();
                Poll::Ready(f(o))
            }
        }
    }
}

unsafe fn fake_clone(data: *const ()) -> RawWaker {
    RawWaker::new(data, &FAKE_VTABLE)
}
unsafe fn fake_wake(_data: *const ()) {

}
unsafe fn fake_wake_by_ref(_data: *const ()) {

}
fn fake_drop(_data: *const()) {

}
const FAKE_VTABLE: RawWakerVTable = RawWakerVTable::new(fake_clone, fake_wake, fake_wake_by_ref, fake_drop);

/**
Polls the future once.

Returns the value if any.  If the future is [Poll::Pending], returns None.

This is primarily of interest in the case you "happen to know" some future will be ready immediately,
and need a way to call it from a sync context.

It is not terribly useful in the case you want to retry polling the future to get a value a second time.
*/
pub fn poll_inline<R,F: Future<Output=R>>(mut future: F) -> Option<R> {
    let raw_waker = RawWaker::new(std::ptr::null(), &FAKE_VTABLE);
    let future = unsafe{Pin::new_unchecked(&mut future)};
    let waker = unsafe{Waker::from_raw(raw_waker)};
    let mut context = Context::from_waker(&waker);
    match future.poll(&mut context) {
        Poll::Ready(r) => Some(r),
        Poll::Pending => None
    }

}
///A future which is not implemented.
///
/// This macro implements [std::future::Future] for any `Output`, by calling `todo!()`.
#[macro_export]
macro_rules! todo_future {
    ($ty:ty) => {
        {
            use core::task::Poll;
            use core::task::Context;
            use std::pin::Pin;
            struct TodoFuture;
            impl Future for TodoFuture {
                type Output = $ty;
                fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                    todo!()
                }
            }
            //expr
            TodoFuture
        }
    }
}