/*! Local primitives*/
use std::fmt::{Display, Formatter};
use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Instant;
use crate::fake_waker::FakeWaker;

mod task;
mod executor;
pub use executor::Executor;

#[derive(Debug)]
pub enum TimeoutError {
    TimedOut
}
impl Display for TimeoutError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("kiruna::sync::block timed out")
    }
}
impl std::error::Error for TimeoutError {}


///A (potentially dumb) way to block on a future.  Returns the future's output.
///
/// There are a lot of reasons why this might be a bad idea.  For starters,
/// if two futures somehow block on each other, they can deadlock.  However,
/// this technique is still sometimes useful for quick prototyping.
///
/// Compare this approach with [crate::test::test_await], a similar approach for tests.  The primary difference is that one panics
/// while this one returns a Result.  I am leaving them separate for now as I have some suspicion implementations may evolve
/// over time.
pub fn block<F: Future>(timeout: std::time::Duration, future: F) -> Result<F::Output,TimeoutError> {
    let fake_waker = Arc::new(FakeWaker);
    let as_waker: Waker = fake_waker.into();
    let mut as_context = Context::from_waker(&as_waker);
    let instant = Instant::now();
    let mut pinned = Box::pin(future);
    while instant.elapsed() < timeout {
        let result = pinned.as_mut().poll(&mut as_context);
        if let Poll::Ready(output) = result {
            return Ok(output)
        }
    }
    Err(TimeoutError::TimedOut)
}

#[test] fn test_block() {
    let m = async {
        "my test string"
    };
    let result = block(std::time::Duration::new(1,0), m).unwrap();
    assert_eq!(result,"my test string");
}
#[test] fn test_block_timeout() {
    use std::pin::Pin;
    //some future that never returns
    struct Never;
    impl Future for Never {
        type Output = ();
        fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
            Poll::Pending
        }
    }
    let result = block(std::time::Duration::from_millis(100), Never);
    assert!(result.is_err());
}