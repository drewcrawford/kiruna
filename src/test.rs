use std::task::{Poll, Waker, Context};
use std::sync::Arc;
use std::future::Future;
use std::time::{Duration, Instant};
use super::fake_waker::FakeWaker;


///A very silly single poll designed for tests
pub fn test_poll<F: Future>(future: F) -> Poll<F::Output> {
    let fake_waker = Arc::new(FakeWaker);
    let as_waker: Waker = fake_waker.into();
    let mut as_context = Context::from_waker(&as_waker);
    let mut pinned = Box::pin(future);
    pinned.as_mut().poll(&mut as_context)
}
///A very silly executor designed for tests.
///
/// Compare this with [sync::block], which is similar, but doesn't panic.
/// I am leaving these implementations separate for now as I suspect they might
/// diverge in the future.
pub fn test_await<F: Future>(future: F, timeout: Duration) -> F::Output {
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