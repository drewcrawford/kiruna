use std::task::{Poll, Waker, Context, Wake};
use std::sync::{Arc, Mutex};
use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc::{channel, Sender};
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

/**
A more cumbersome version of [test_poll] that you can call multiple times on the same Future.

You must pin the future before calling this method. */
pub fn test_poll_pin<F: Future>(future: &mut Pin<&mut F>) -> Poll<F::Output> {
    let move_pin = future.as_mut();
    let fake_waker = Arc::new(FakeWaker);
    let as_waker: Waker = fake_waker.into();
    let mut as_context = Context::from_waker(&as_waker);
    move_pin.poll(&mut as_context)
}
///A very silly executor designed for tests.
///
/// Compare this with [crate::sync::block], which is similar, but doesn't panic.
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

pub struct SimpleWaker(Mutex<Sender<()>>);
impl Wake for SimpleWaker {
    fn wake(self: Arc<Self>) {
        self.0.lock().unwrap().send(()).unwrap();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.lock().unwrap().send(()).unwrap();
    }
}

/**
Another executor designed for tests.

Instead of polling in a busyloop, this 'executor' polls the future sparsely, only
when it is required by the API contract.  This behaves more like a real executor, and
may uncover different bugs.
*/
pub fn sparse_await<F: Future>(future: F, timeout: Duration) -> F::Output {
    let (sender,receiver) = channel();
    let waker = Arc::new(SimpleWaker(Mutex::new(sender)));
    let as_waker: Waker = waker.into();
    let mut as_context = Context::from_waker(&as_waker);
    let started = Instant::now();
    let mut pinned = Box::pin(future);
    loop {
        if let Poll::Ready(output) = pinned.as_mut().poll(&mut as_context) {
            return output;
        }
        let elapsed = started.elapsed();
        match timeout.checked_sub(elapsed) {
            Some(duration) => {
                receiver.recv_timeout(duration).unwrap();
            }
            None => {
                panic!("Deadline exceeded!");
            }
        }
    }
}