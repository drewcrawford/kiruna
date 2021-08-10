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
///A very silly executor designed for tests.
pub fn test_await<F: Future>(future: F, timeout: Duration) -> F::Output{
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