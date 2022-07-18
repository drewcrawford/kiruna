/*!
Defines work-stealing bins. */

/*
A brief discussion of why this isn't using GCD.

You can see swift's implementation of concurrency here https://github.com/apple/swift/blob/main/stdlib/public/Concurrency/DispatchGlobalExecutor.inc
(or at least, a little of it.)

the tl;dr is it prefers the hidden function `dispatch_async_swift_job`, which as far as I know
is not discussed anywhere.  If you googled it and ended up in this comment, send me fan mail :-)
It does not appear in libdispatch-1271.40.12, and I assume we are not going to see it any time soon.

When this is not available, such as windows or backdeployed concurrency, it falls back to dispatch_async_f on a queue.  This queue
1.  Is created by swift (e.g., concurrent)
2.  Is configured by the private `dispatch_queue_set_width` with the argument `-3` (documented to be named `DISPATCH_QUEUE_WIDTH_MAX_LOGICAL_CPUS`)  According
to queue_private.h, this function is deprecated "and will be removed in a future release".

The `apply` strategy is not preferred by Swift, and I have reason to think its performance is not great.  It is *probably*
ABI-stable, but I'm not confident it is appstore-safe for example.

Using the dispatch_async_swift_job is probably not a good bet either.  I am not so confident it is ABI-stable as parts
of the swift runtime are delivered with the OS, so I am not confident the arguments are OK as ABI.

Another possibility would be to actually write this executor in Swift, and then we can lean on Swift's executor
implementation.  However then we have the whole Swift runtime to deal with.

I think reimplementing this and interfacing with the OS at the thread level makes the most sense here, even on macOS.
 */
use std::ffi::c_void;
use std::future::Future;
use std::sync::atomic::{Ordering};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::time::{Duration, Instant};
use once_cell::sync::OnceCell;
use crate::platform::*;

/*
A brief discussion of this dependency.

I looked into writing my own atomic queue.  I even designed one that seems in a cursory examination to be better
than the widely-used michael-scott scheme (it is easier to analyze at least, which makes it a good fit for kiruna's goals).

It seems that algorithms in the class, including my design, need some actual non-arc GC.  There's a window of time
where a reference count has reached zero and some other thread is trying to acquire it; to solve this you have to defer
the deallocation 'awhile', or have some scheme where you tag pointers in the unused bits to fit them in a word,
and decide not to reference them because they have old tags, etc.

I think writing a garbage collector is a little outside my scope at present, and the best queue based on an existing gc is going to
be the crossbeam channel.  I think it is not totally optimal on weakly-ordered memory machines but.

I also looked briefly into flume, which is perhaps kiruna's "flavor" of simple dependency.
They have some impressive benchmarks but I was not immediately able to understand how to replicate their mpmc tests.
A brief examination also suggests they use locks, which I am somewhat skeptical
about for the workload.  I would be willing to believe benchmarks if I could replicate them, but I cannot.
 */
use crossbeam_channel::{Receiver, Sender};

use crate::global::{GlobalState, ThreadCounts};
use crate::HeapFuture;

#[derive(Copy,Clone)]
pub enum WhichBin {
    UserWaiting,
}


enum ThreadMessage {
    Work(HeapFuture),
    ///This message is occasionally sent to threads to allow them to think about dying
    Idle,
}

/**
A bin is an object within which we implement work-stealing.
*/
pub struct Bin {
    sender: Sender<ThreadMessage>,
    receiver: Receiver<ThreadMessage>,
    which_bin: WhichBin,
    _idle_timer: Timer,
}

const USER_WAITING_MIN_THREADS:u16 = 1;


struct UserWaitingBinWaker {
}
impl UserWaitingBinWaker {
    fn waker() -> Waker {
        unsafe{Waker::from_raw(Self::raw_waker())}
    }
    fn clone(_data: *const ()) -> RawWaker {
        todo!()
    }
    fn wake(_data: *const ()) {
        todo!()
    }
    fn wake_by_ref(_data: *const ()) {
        todo!()
    }
    fn drop(_data: *const ()) {
        //todo!() nothing to do at the moment
    }

    const VTABLE: RawWakerVTable = RawWakerVTable::new(Self::clone, Self::wake, Self::wake_by_ref, Self::drop);

    fn raw_waker() -> RawWaker {
        RawWaker::new(std::ptr::null(), &Self::VTABLE)
    }
}
#[cfg(feature="thread_stories")]
macro_rules! log_time {
    (
        $($arg:tt)*
    ) => {
        log_time($($arg)*)
    }
}
#[cfg(not(feature = "thread_stories"))]
macro_rules! log_time {
    (
        $($arg:tt)*
    ) => {
    }
}
#[cfg(feature="thread_stories")]
fn log_time(str: String) {
    let instant = Instant::now();
    println!("{instant:?}:{str}");
}

fn thread_user_waiting_entrypoint_fn() {
    /*
    The amount of time a thread will wait before shutting down.

    Note that this value is considered in the context of a local thread, but threads only evaluate this when asked by the bin.
    For a thread to shutdown, it must both be idle for the specified time and be asked to shutdown by the bin.
     */
    #[cfg(test)]
    const TARGET_IDLE_TIME: Duration = Duration::from_millis(100);
    #[cfg(not(test))]
    const TARGET_IDLE_TIME: Duration = Duration::from_secs(1);

    let bin = Bin::user_waiting();
    let waker = UserWaitingBinWaker::waker();
    let mut context = Context::from_waker(&waker);
    let mut last_useful = Instant::now();
    #[cfg(feature="thread_stories")]
        use rand::{Rng, SeedableRng};
    #[cfg(feature = "thread_stories")]
        use rand::distributions::Alphanumeric;
    #[cfg(feature = "thread_stories")]
        let thread_debug_id: String =  rand::rngs::StdRng::from_entropy().sample_iter(&Alphanumeric).take(5).map(char::from).collect();

    log_time!(format!("thread_user_waiting_entrypoint_fn {thread_debug_id:?}"));
    loop {
        log_time!(format!("worker thread {thread_debug_id:?} recv"));
        let f = bin.receiver.recv().unwrap();
        log_time!(format!("worker thread {thread_debug_id:?} recv done"));

        match f {
            ThreadMessage::Idle => {
                log_time!(format!("worker thread {thread_debug_id:?} sees idle message"));
                let last_check = last_useful.elapsed();
                if last_check > TARGET_IDLE_TIME {
                    let update_result = GlobalState::global().update_thread_counts(|counts| {
                        if counts.user_waiting > USER_WAITING_MIN_THREADS {
                            counts.user_waiting -= 1;
                        }
                    });
                    if update_result.is_ok() {
                        log_time!(format!("worker thread {thread_debug_id:?} shutdown"));
                        return;
                    }
                    else {
                        log_time!(format!("worker thread {thread_debug_id:?} WONT shutdown as it's the only thread"));
                    }
                }
                else {
                    log_time!(format!("worker thread {thread_debug_id:?} WONT shutdown as it was recently used {last_check:?}"));

                }
            }
            ThreadMessage::Work(mut future) => {
                log_time!(format!("worker thread {thread_debug_id:?} doing work"));
                match future.as_mut().poll(&mut context) {
                    Poll::Ready(_) => {
                        //done!
                    }
                    Poll::Pending => {
                        todo!()
                    }
                }
                log_time!(format!("worker thread {thread_debug_id:?} done with work"));
                last_useful = Instant::now();
            }

        }

    }
}

extern "C" fn user_waiting_timer_callback(_arg: *mut c_void) {
    //We want to send out as many messages as reasonable.
    //Note that we don't have to get this exactly
    log_time!(format!("timer"));
    let thread_counts: ThreadCounts = GlobalState::global().read_thread_counts();
    //the idea here is we gently avoid filling the queue.  The actual policy tends to be enforced by the workers.
    //This is because, in theory, the timer can execute faster than workers.  It's possible for multiple timers
    //to run before a worker is listening.
    if thread_counts.user_waiting > USER_WAITING_MIN_THREADS { //leave some threads
        log_time!(format!("sending idle message to {} of {} threads",thread_counts.user_waiting/2,thread_counts.user_waiting));
        for _ in 0..thread_counts.user_waiting / 2 {
            //ask up to half the threads to shut down.
            log_time!(format!("send idle message"));
            Bin::user_waiting().sender.send(ThreadMessage::Idle).unwrap();
        }
    }

}

impl Bin {
    pub fn user_waiting() -> &'static Bin {
        static USER_WAITING_BIN: OnceCell<Bin> = OnceCell::new();
        
        USER_WAITING_BIN.get_or_init(|| {
            let (sender, receiver) = crossbeam_channel::bounded(1_000_000);
            Bin {
              sender: sender,
                receiver,
                which_bin: WhichBin::UserWaiting,
                _idle_timer: Timer::new(user_waiting_timer_callback, Duration::from_secs(10), Duration::from_secs(60))
          }  
        })
    }

    //homogeneous futures array
    pub fn spawn<const LENGTH: usize, F: Future<Output=()> + Send + 'static>(&'static self, future: [F; LENGTH]) {
        self.enforce_spare_thread_policy(LENGTH);
        for task in future {
            let message = ThreadMessage::Work(Box::pin(task));
            self.sender.send(message).unwrap();
        }
    }

    //heterogeneous array
    pub fn spawn_mixed<const LENGTH: usize>(&'static self, futures: [HeapFuture; LENGTH]) {
        self.enforce_spare_thread_policy(LENGTH);
        for task in futures {
            let message = ThreadMessage::Work(Box::pin(task));
            self.sender.send(message).unwrap();
        }
    }

    pub fn spawn_without_hint(&'static self, future: HeapFuture) {
        let message = ThreadMessage::Work(future);
        self.sender.send(message).unwrap();
    }

    pub fn enforce_spare_thread_policy(&'static self, coming_soon: usize) {
        let state = GlobalState::global();
        //the number of threads we would want to be active, without any knowledge of what is happening in the system
        let proposed_threads = coming_soon.min(state.physical_cpus as usize) as u16;

        let old_threadcount = state.thread_counts.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |thread_counts| {
            let mut thread_count: ThreadCounts = thread_counts.into();
            if thread_count.user_waiting >= proposed_threads {
                None
            }
            else {
                thread_count.user_waiting = proposed_threads;
                Some(thread_count.into())
            }
        });
        match old_threadcount {
            Ok(old_threads) => {
                //In this case, we need to launch some threads.  We promised we would go up to proposed_threads, so the launch amount is
                let old_threadcount: ThreadCounts = old_threads.into();
                let launch_amount = proposed_threads - old_threadcount.user_waiting;
                log_time!(format!("launching {launch_amount} new threads"));
                for _ in 0..launch_amount {
                    spawn_thread(self.which_bin, thread_user_waiting_entrypoint_fn )
                }
            }
            Err(_) => {
                //in this case, our function returned None.  We have enough threads, don't launch anymore.
            }
        }

    }
}

#[cfg(test)] mod tests {
    use std::time::{Duration};
    use crate::{Bin, HeapFuture};
    use crate::bin::{user_waiting_timer_callback};
    use crate::global::GlobalState;


    #[test] fn thread_policy() {
        let bin = Bin::user_waiting();
        //more cores than I have.  Your mileage may vary
        let all_tasks = [
            Box::pin(async move {}) as HeapFuture,
            Box::pin(async move {}),
            Box::pin(async move {}),
            Box::pin(async move {}),
            Box::pin(async move {}),
            Box::pin(async move {}),
            Box::pin(async move {}),
            Box::pin(async move {}),
            Box::pin(async move {}),
            Box::pin(async move {}),
            Box::pin(async move {}),
        ];
        bin.spawn_mixed(all_tasks);

        let thread_counts = GlobalState::global().read_thread_counts();
        assert!(thread_counts.user_waiting == GlobalState::global().physical_cpus);

        //wait for threads to reach their idle state
        std::thread::sleep(Duration::from_millis(250));
        //simulate varoius timer fires
        for _ in 0..50 {
            user_waiting_timer_callback(std::ptr::null_mut());
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
        assert_eq!(GlobalState::global().read_thread_counts().user_waiting, 1);
    }
}
