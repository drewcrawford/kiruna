use std::future::Future;
use std::fmt::Debug;
use std::pin::Pin;
use std::task::{Context, Poll};

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