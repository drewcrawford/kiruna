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
use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use std::time::{Duration, Instant};
use once_cell::sync::OnceCell;
use crate::platform::*;
use spawn::{MicroPriority, spawn_thread};

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
use crossbeam_channel::{Receiver, select, Sender};
use priority::Priority;

use crate::global::{GlobalState, ThreadCounts};
use crate::stories::Story;

#[derive(Copy,Clone)]
pub enum WhichBin {
    UserWaiting,
}

pub type SimpleJob = Box<dyn FnOnce() -> () + Send>;

enum ThreadMessage {
    ///In cases where we are confident the fn won't resume, we can be substantially faster
    Simple(SimpleJob),
    Work(Arc<OurFuture>),
    ///This message is occasionally sent to threads to allow them to think about dying
    Idle,
}

/**
A bin is an object within which we implement work-stealing.
*/
pub struct Bin {
    sender: Sender<ThreadMessage>,
    receiver: Receiver<ThreadMessage>,
    //The "preferred" channel is a channel for higher priority work within the bin.
    //Typically, this is for futures that have been polled once.  We want to repoll them again
    //quickly as they are likely to be complete.
    preferred_sender: Sender<ThreadMessage>,
    preferred_receiver: Receiver<ThreadMessage>,
    which_bin: WhichBin,
    _idle_timer: Timer,
}

const USER_WAITING_MIN_THREADS:u16 = 1;

/**
There is a fundamental overlapping problem with futures.  It is.

1.  The future is polled.  This requires an exclusive pointer.
2.  As part of its poll, you pass it a context (which contains a waker)
3.  The waker can be woken
4.  The correct thing to do to handle the wake is to poll the future.  However, this can overlap with 1,
    which is incorrect.

To solve this, we ned a type that can lock.

This is implemented as a generic type.  It will be boxed, typically inside an Arc.
# Design note
I tried a version of this that takes the future generically, and then stores the contents inline,
using a trait to erase.  The problem here is that the trait object will be a fat pointer.  Then when we try to pass
one pointer to our RawWaker, it is too big.  For this reason, we really do need a concerete type I expect.
*/
struct OurFuture {
    //note: ALL access to these fields must be checked for races!!!
    underlying: UnsafeCell<Pin<Box<dyn Future<Output=()> + Send>>>,
    locked: AtomicBool,
}
//note: ALL access to fields must be checked for races!!!
unsafe impl Sync for OurFuture {}
impl OurFuture {
    fn new(future: Pin<Box<dyn Future<Output=()> + Send>>) -> Self {
        Self { underlying: UnsafeCell::new(future), locked: AtomicBool::new(false) }
    }
    //not an implementation of future since we don't require pin or mut
    fn poll(&self, cx: &mut Context<'_>) -> Poll<()> {
        unsafe {
            match self.locked.compare_exchange(false, true, Ordering::Acquire /* crit section after */, Ordering::Relaxed) {
                Ok(..) => {
                    let underlying: Pin<&mut dyn Future<Output=()>> = (&mut *self.underlying.get()).as_mut();
                    let r = underlying.poll(cx);
                    self.locked.store(false, Ordering::Release); //crit section before
                    r
                }
                Err(..) => {
                    //schedule a retry sometime when the lock is available
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            }
        }
    }
}


struct UserWaitingBinWaker {
}
impl UserWaitingBinWaker {
    /**
    Returns a borrowed, future.

    # Safety
    You must pass a data pointer that was generated by calling [Self::raw_waker].
    The type will be returned with unbounded lifetime.  However, it is only valid for the lifetime of the underlying waker.
     */
    unsafe fn raw_waker<'unbounded>(future: Arc<OurFuture>) -> (RawWaker,&'unbounded OurFuture) {
        let into_raw = Arc::into_raw(future);
        let r = RawWaker::new(into_raw as *const (), &Self::VTABLE);
        let f = & *into_raw;
        (r,f)
    }
    fn clone(data: *const ()) -> RawWaker {
        unsafe{Arc::increment_strong_count(data as *const _)};
        RawWaker::new(data, &Self::VTABLE)
    }
    fn wake(data: *const ()) {
        let arc_future = unsafe{Arc::from_raw(data as *const OurFuture)};
        let message = ThreadMessage::Work(arc_future);
        Bin::user_waiting().preferred_sender.send(message).unwrap();
    }
    fn wake_by_ref(data: *const ()) {
        unsafe { Arc::increment_strong_count(data as *const OurFuture)}
        let b = unsafe{Arc::from_raw(data as *const OurFuture)};
        let message = ThreadMessage::Work(b);
        Bin::user_waiting().preferred_sender.send(message).unwrap();
    }
    fn drop(data: *const ()) {
        let _drop = unsafe{Arc::from_raw(data as *const OurFuture)};
    }
    const VTABLE: RawWakerVTable = RawWakerVTable::new(Self::clone, Self::wake, Self::wake_by_ref, Self::drop);
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
    let mut last_useful = Instant::now();
    let story = Story::new();

    story.log(format!("thread_user_waiting_entrypoint_fn"));
    loop {
        story.log(format!("worker thread recv"));
        //first, try receiving on the preferred channel
        let task = match bin.preferred_receiver.recv_timeout(Duration::ZERO) {
            Ok(task) => task,
            Err(..) => {
                //now select on them both
                select! {
                    recv(bin.preferred_receiver) -> msg => msg.unwrap(),
                    recv(bin.receiver) -> msg => msg.unwrap()
                }
            }
        };
        story.log(format!("worker thread recv done"));

        match task {
            ThreadMessage::Idle => {
                story.log(format!("worker thread sees idle message"));
                let last_check = last_useful.elapsed();
                if last_check > TARGET_IDLE_TIME {
                    let update_result = GlobalState::global().update_thread_counts(|counts| {
                        if counts.user_waiting > USER_WAITING_MIN_THREADS {
                            counts.user_waiting -= 1;
                        }
                    });
                    if update_result.is_ok() {
                        story.log(format!("worker thread shutdown"));
                        return;
                    }
                    else {
                        story.log(format!("worker thread WONT shutdown as it's the only thread"));
                    }
                }
                else {
                    story.log(format!("worker thread WONT shutdown as it was recently used {last_check:?}"));

                }
            }
            ThreadMessage::Work(future) => {
                unsafe {
                    //safety: unsafe_future is valid for the lifetime of waker only
                    let (waker, unsafe_future) = UserWaitingBinWaker::raw_waker(future);
                    //safety: contract ought to be correctly implemented
                    let full_waker =  Waker::from_raw(waker);
                    let mut context = Context::from_waker(&full_waker);
                    story.log(format!("worker thread doing work"));
                    match unsafe_future.poll(&mut context) {
                        Poll::Ready(_) => {
                            //done!
                        }
                        Poll::Pending => {
                            //wait for context I presume
                        }
                    }
                    std::mem::drop(full_waker); //explicit drop here
                    std::mem::drop(unsafe_future);
                }
                story.log(format!("worker thread done with work"));
                last_useful = Instant::now();
            }
            ThreadMessage::Simple(job) => {
                story.log(format!("worker thread doing work"));
                job();
                story.log(format!("worker thread done with work"));
                last_useful = Instant::now();

            }

        }

    }
}
#[cfg(target_os = "windows")]
extern "system" fn user_waiting_timer_callback_thunk(_a: *mut c_void, _b: *mut c_void, _c: *mut c_void) {
    user_waiting_timer_callback()
}
#[cfg(target_os = "macos")]
extern "C" fn user_waiting_timer_callback_thunk(_a: *mut c_void) {
    user_waiting_timer_callback()
}

fn user_waiting_timer_callback() {
    //We want to send out as many messages as reasonable.
    //Note that we don't have to get this exactly
    let story = Story::new();
    story.log(format!("timer"));
    let thread_counts: ThreadCounts = GlobalState::global().read_thread_counts();
    //the idea here is we gently avoid filling the queue.  The actual policy tends to be enforced by the workers.
    //This is because, in theory, the timer can execute faster than workers.  It's possible for multiple timers
    //to run before a worker is listening.
    if thread_counts.user_waiting > USER_WAITING_MIN_THREADS { //leave some threads
        story.log(format!("sending idle message to {} of {} threads",thread_counts.user_waiting/2,thread_counts.user_waiting));
        for _ in 0..thread_counts.user_waiting / 2 {
            //ask up to half the threads to shut down.
            story.log(format!("send idle message"));
            Bin::user_waiting().sender.send(ThreadMessage::Idle).unwrap();
        }
    }

}

impl Bin {
    pub fn user_waiting() -> &'static Bin {
        static USER_WAITING_BIN: OnceCell<Bin> = OnceCell::new();
        
        USER_WAITING_BIN.get_or_init(|| {
            let (sender, receiver) = crossbeam_channel::bounded(100);
            let (preferred_sender,preferred_receiver) = crossbeam_channel::bounded(100);
            Bin {
              sender: sender,
                receiver,
                preferred_sender, preferred_receiver,
                which_bin: WhichBin::UserWaiting,
                _idle_timer: Timer::new(user_waiting_timer_callback_thunk, Duration::from_secs(10), Duration::from_secs(60))
          }  
        })
    }

    //homogeneous futures array
    pub fn spawn<const LENGTH: usize, F: Future<Output=()> + Send + 'static>(&'static self, future: [F; LENGTH]) {
        self.enforce_spare_thread_policy(LENGTH);
        for task in future {
            let our_future = Arc::new(OurFuture::new(Box::pin(task)));
            let message = ThreadMessage::Work(our_future);
            self.sender.send(message).unwrap();
        }
    }

    pub fn spawn_mixed_simple<const LENGTH: usize>(&'static self, jobs: [SimpleJob; LENGTH]) {
        self.enforce_spare_thread_policy(LENGTH);
        for job in jobs {
            let message = ThreadMessage::Simple(job);
            self.sender.send(message).unwrap();
        }
    }

    //heterogeneous array
    //design note.  We require boxed for 'static support.
    pub fn spawn_mixed<const LENGTH: usize>(&'static self, futures: [Pin<Box<(dyn Future<Output=()> + Send)>>; LENGTH]) {
        self.enforce_spare_thread_policy(LENGTH);
        for task in futures {
            let our_task = Arc::new(OurFuture::new(task));
            let message = ThreadMessage::Work(our_task);
            self.sender.send(message).unwrap();
        }
    }

    pub fn spawn_without_hint<F: Future<Output=()> + Send + 'static>(&'static self, future: F) {
        let our_task = Arc::new(OurFuture::new(Box::pin(future)));
        let message = ThreadMessage::Work(our_task);
        self.sender.send(message).unwrap();
    }

    pub fn spawn_simple_without_hint(&'static self, future: SimpleJob) {
        let message = ThreadMessage::Simple(future);
        self.sender.send(message).unwrap();
    }

    pub fn enforce_spare_thread_policy(&'static self, coming_soon: usize) {
        let story = Story::new();
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
                story.log(format!("launching {launch_amount} new threads"));
                for _ in 0..launch_amount {
                    let priority = match self.which_bin {
                        WhichBin::UserWaiting => {Priority::UserWaiting}
                    };
                    spawn_thread(priority,  MicroPriority::NEW, thread_user_waiting_entrypoint_fn)
                }
            }
            Err(_) => {
                //in this case, our function returned None.  We have enough threads, don't launch anymore.
            }
        }

    }
}

#[cfg(test)] mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll,Waker};
    use std::time::{Duration, Instant};
    use crate::bin::{user_waiting_timer_callback};
    use crate::global::GlobalState;
    use std::sync::Mutex;
    use super::Bin;

    ///A future that takes exactly 100ms
    struct SillyFuture {
        waker: Arc<Mutex<Option<Waker>>>,
        done: Arc<Mutex<bool>>,
    }
    impl SillyFuture {
        fn new() -> Self {
            Self {
                waker: Arc::new(Mutex::new(None)),
                done: Arc::new(Mutex::new(false))
            }
        }
    }
    impl Future for SillyFuture {
        type Output = ();

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            println!("poll!");
            let move_waker = self.waker.clone();
            let move_done = self.done.clone();
            std::thread::spawn(move || {
               *move_done.lock().unwrap() = true;
                move_waker.lock().unwrap().take().unwrap().wake();
            });
            let done = *self.done.lock().unwrap();
            if done {
                Poll::Ready(())
            }
            else {
                *self.waker.lock().unwrap() = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
    #[test] fn test_yield() {
        let bin = Bin::user_waiting();
        let s = SillyFuture::new();
        let done = s.done.clone();
        let resumed = Arc::new(AtomicBool::new(false));
        let resumed_clone = resumed.clone();
        bin.spawn([
            async move {
                s.await;
                resumed_clone.store(true, Ordering::Relaxed);
            }
        ]);
        std::thread::sleep(Duration::from_millis(1000));
        assert!(*done.lock().unwrap());
        assert!(resumed.load(Ordering::Relaxed));
    }


    #[test] fn thread_policy() {
        let bin = Bin::user_waiting();
        //more cores than I have.  Your mileage may vary
        let all_tasks = [
            Box::pin(async move {}) as Pin<Box<(dyn Future<Output=()> + Send)>>,
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
        std::thread::sleep(Duration::from_millis(500));

        //simulate various timer fires
        //note that this can run in parallel with other tests, so we need to wait long enough
        //for all of them to complete, or at least in test mode.
        let started_waiting = Instant::now();
        while GlobalState::global().read_thread_counts().user_waiting != 1 {
            user_waiting_timer_callback();
            if started_waiting.elapsed() > std::time::Duration::from_secs(20) {
                panic!("Never spun down");
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let time = started_waiting.elapsed();
        println!("finished thread_poilcy in {time:?}");
    }
}
