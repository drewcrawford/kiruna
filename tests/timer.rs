#![cfg(feature="sync")]
use std::future::Future;
use std::task::{Waker, Context, Poll};
use std::pin::Pin;
use std::time::Duration;
use std::thread::{spawn, sleep};
use std::sync::{Arc, Mutex};
use kiruna::sync::Executor;

struct SharedState {
    completed: bool,
    returned_ready: bool,
    waker: Option<Waker> //MaybeUnint get_ref was stabilized in 1.55 but not yet available here
}

///https://rust-lang.github.io/async-book/02_execution/03_wakeups.html
struct TimerFuture {
    shared_state: Arc<Mutex<SharedState>>
}
impl Future for TimerFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        println!("TimerFuture poll");
        let mut shared_state = self.shared_state.lock().unwrap();
        shared_state.waker = Some(cx.waker().to_owned());
        if shared_state.completed {
            println!("TF is completed");
            assert!(!shared_state.returned_ready);
            shared_state.returned_ready = true;
            Poll::Ready(())
        }
        else {
            println!("TF is not completed");
            //could be optimized with will_wake
            Poll::Pending
        }
    }
}
impl TimerFuture {
    fn new(duration: Duration) -> TimerFuture {
        //This is kinda bad design.  ideally, we wouldn't do anything until polled, but this is just a test so...
        let shared_state = Arc::new(Mutex::new(SharedState {
            completed: false,
            returned_ready:false,
            waker: None
        }));
        let cloned_state = shared_state.clone();
        spawn(move || {
            sleep(duration);
            let mut shared_state = cloned_state.lock().unwrap();
            shared_state.completed = true;
            shared_state.waker.as_ref().expect("Must wait on this future before completing it").wake_by_ref();

        });
        TimerFuture {
            shared_state
        }
    }
}
impl Drop for TimerFuture {
    fn drop(&mut self) {
        println!("Drop TimerFuture");
    }
}

#[test]
fn timer() {
    let mut executor = Executor::new();
    {
        let future = TimerFuture::new(Duration::from_secs(1));
        executor.spawn(future);
    }
    {
        executor.drain();

    }
}