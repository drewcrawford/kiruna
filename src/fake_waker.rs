use std::task::{Wake, Poll, Waker, Context};
use std::sync::Arc;
use std::future::Future;
use std::time::{Duration, Instant};

//fake waker purely for debug purposes
pub(crate) struct FakeWaker;
impl Wake for FakeWaker {
    fn wake(self: Arc<Self>) {
        //nothing
    }
}
impl FakeWaker {
    pub fn new_waker() -> Waker {
        Arc::new(FakeWaker).into()
    }
}

pub fn toy_poll<F: Future>(future: F) -> Poll::<F::Output> {
    //println!("await {:?}",self.0);
    let fake_waker = Arc::new(FakeWaker);
    let as_waker: Waker = fake_waker.into();
    let mut as_context = Context::from_waker(&as_waker);
    Box::pin(future).as_mut().poll(&mut as_context)
}

pub fn toy_await<F: Future>(future: F, timeout: Duration) -> F::Output{
    let fake_waker = Arc::new(FakeWaker);
    let as_waker: Waker = fake_waker.into();
    let mut as_context = Context::from_waker(&as_waker);
    let instant = Instant::now();
    let mut pinned = Box::pin(future);
    while instant.elapsed() < timeout {
        let result = pinned.as_mut().poll(&mut as_context);
        if let Poll::Ready(output) = result {
            return output;
        }
    }
    panic!("Future never arrived!");
}