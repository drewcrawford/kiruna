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
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::time::Duration;
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

#[derive(Copy,Clone)]
pub enum WhichBin {
    UserWaiting,
}

type HeapFuture = Pin<Box<dyn Future<Output=()> + Send>>;

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
    idle_timer: Timer,
}

///Special statistics type, can be encoded into a u64 for atomic ops
struct ThreadCounts {
    user_waiting: u16,
}
impl ThreadCounts {
    fn new() -> ThreadCounts { Self { user_waiting: 0}}
}
impl From<u64> for ThreadCounts {
    fn from(u: u64) -> Self {
        ThreadCounts {
            user_waiting: (u & 0xffffffff) as u16
        }
    }
}
impl From<ThreadCounts> for u64 {
    fn from(t: ThreadCounts) -> Self {
        t.user_waiting as u64
    }
}

struct GlobalState {
    physical_cpus: u16,
    //encoding of ThreadCounts type.
    thread_counts: AtomicU64,
}
impl GlobalState {
    fn global() -> &'static GlobalState {
        static GLOBAL_STATE: OnceCell::<GlobalState> = OnceCell::new();
        GLOBAL_STATE.get_or_init( ||{
            GlobalState::new()
        })
    }
    fn new() -> GlobalState {
        let physical_cpus = physical_cpus();
        GlobalState {
            physical_cpus: physical_cpus,
            thread_counts: AtomicU64::new(ThreadCounts::new().into()),
        }
    }
}

struct UserWaitingBinWaker {
}
impl UserWaitingBinWaker {
    fn waker() -> Waker {
        unsafe{Waker::from_raw(Self::raw_waker())}
    }
    fn clone(data: *const ()) -> RawWaker {
        todo!()
    }
    fn wake(data: *const ()) {
        todo!()
    }
    fn wake_by_ref(data: *const ()) {
        todo!()
    }
    fn drop(data: *const ()) {
        todo!()
    }

    const VTABLE: RawWakerVTable = RawWakerVTable::new(Self::clone, Self::wake, Self::wake_by_ref, Self::drop);

    fn raw_waker() -> RawWaker {
        RawWaker::new(std::ptr::null(), &Self::VTABLE)
    }
}

fn thread_user_waiting_entrypoint_fn() {
    let bin = Bin::user_waiting();
    let waker = UserWaitingBinWaker::waker();
    let mut context = Context::from_waker(&waker);

    loop {
        let mut f = bin.receiver.recv().unwrap();
        match f {
            ThreadMessage::Idle => {
                todo!()
            }
            ThreadMessage::Work(mut future) => {
                match future.as_mut().poll(&mut context) {
                    Poll::Ready(_) => {
                        //done!
                    }
                    Poll::Pending => {
                        todo!()
                    }
                }
            }

        }

    }
}

extern "C" fn user_waiting_timer_callback(arg: *mut c_void) {
    println!("timer");
    Bin::user_waiting().sender.send(ThreadMessage::Idle).unwrap();
}

impl Bin {
    pub fn user_waiting() -> &'static Bin {
        static USER_WAITING_BIN: OnceCell<Bin> = OnceCell::new();
        
        USER_WAITING_BIN.get_or_init(|| {
            let (sender, receiver) = crossbeam_channel::bounded(10);
            Bin {
              sender: sender,
                receiver,
                which_bin: WhichBin::UserWaiting,
                idle_timer: Timer::new(user_waiting_timer_callback, Duration::from_secs(1), Duration::from_secs(30))
          }  
        })
    }

    pub fn spawn<const LENGTH: usize, F: Future<Output=()> + Send + 'static>(&'static self, future: [F; LENGTH]) {
        self.enforce_spare_thread_policy(LENGTH);
        for task in future {
            let message = ThreadMessage::Work(Box::pin(task));
            self.sender.send(message).unwrap();
        }
    }

    fn enforce_spare_thread_policy(&'static self, coming_soon: usize) {
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
                println!("launching {launch_amount} new threads");
                for t in 0..launch_amount {
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
    use std::time::Duration;
    use crate::Bin;

    #[test] fn test_timer() {
        let bin = Bin::user_waiting();
        std::thread::sleep(Duration::from_secs(10));
    }
}
