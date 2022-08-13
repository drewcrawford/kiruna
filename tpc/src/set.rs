/*!
A set is a collection of tasks that execute in parallel.

The primary way to interact with a set is through a [Guard].
The [Guard] acts as a future that polls the set as a whole;
when the Guard becomes [Poll::Ready], the entire set has completed.

# Design

The rationale for this design may not be completely obvious.  For example, doesn't `kiruna::join` do something pretty similar?

Yes and no.  `join` does glue together multiple futures, so in that sense it seems similar.  And for jobs that are largely
IO bound, it is pretty similar.  However, for CPU-bound work, the situation is a bit different.

`join` is 'just' a future, so when it is sent to the executor it is merely polled, e.g. initially on some particular thread.
Typically polling that future will poll all its child futures, e.g. on the same thread as originally.  This means your `joined`
program, while possibly concurrent, is single-threaded, even on a multi-threaded executor.

It would be possible to design a scheme where parallel executors are snooping for futures like `join` and have some way to 'unpack'
them.  This would be nonstandard, but kiruna joins could be designed to work that way.

The problem is that unpacking every join into a threadpool is not always the right thing to do.  In fact in most common cases,
where you are joining a bunch of IO, one thread is going to be faster than fanning it out.  On my system, parallelism only
starts to make sense when a workload is many tens of microseconds (maybe even hundreds of microseconds).

For that reason, we have the sets.  They are designed around the usecase that you want to run the work in parallel, and you have
jobs that are tens or hundreds of microseconds.  sets always run on the tpc executor, although the Guard future itself
can be run on any executor.
*/
pub mod vec;

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::{Context, Poll};
use atomic_waker::AtomicWaker;
use priority::Priority;
use crate::{Executor};
use crate::bin::SimpleJob;
use crate::stories::Story;

type AtomicSpawned = AtomicU32;
struct SharedSet {
    children_remaining: AtomicSpawned,
    waker: AtomicWaker,
}

struct Child<F> {
    shared: Arc<SharedSet>,
    inner:F,
    done: bool
}
impl<F: Future<Output=()>> Future for Child<F> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        /*
        extract a series of separate, non-overlapping fields
         */
        let (tmp_shared,mut_done, pin_child) = unsafe {
            let self_mut = self.get_unchecked_mut();
            let tmp_shared = &self_mut.shared;
            let mut_done = &mut self_mut.done;
            let pin_child = Pin::new_unchecked(&mut self_mut.inner); //has not moved
            (tmp_shared, mut_done, pin_child)
        };
        match pin_child.poll(cx) {
            Poll::Ready(_) => {
                let r = tmp_shared.children_remaining.fetch_sub(1, Ordering::Release);
                if r == 1 {
                    tmp_shared.waker.wake();
                }
                assert!(!*mut_done, "Polled after done");
                *mut_done = true;
                Poll::Ready(())
            }
            Poll::Pending => {
                //we need to shuttle between our context and the main one...
                todo!()
            }
        }
    }
}


enum State<V> {
    NotSpawned(V),
    Spawned(Arc<SharedSet>),
    Done,
    Invalid
}

struct InternalGuard<V> {
    state: State<V>,
    priority: priority::Priority,
    story: Story,
}

impl<'a, F,V: IntoIterator<Item=F> + Unpin> Future for InternalGuard<V> where Self: 'a, F: Future<Output=()> + Send + 'a {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        fn poll_thunk(set: &Arc<SharedSet>) -> Poll<()> {
            let load = set.children_remaining.load(Ordering::Acquire);
            if load == 0 {
                Poll::Ready(())
            }
            else {
                Poll::Pending
            }
        }
        let mut peek_state = State::Invalid;
        std::mem::swap(&mut self.state, &mut peek_state);
        match peek_state {
            State::NotSpawned(tasks) => {
                self.story.log("Set spawning".to_string());
                //collect the futures up front.  This is because we want to let the child perform the wake to reduce spurious wakes.
                //but each child needs to know how many children to expect
                let tasks: Vec<_> = tasks.into_iter().collect();
                let coming_soon = tasks.len();
                let bin = Executor::global().bin_for(self.priority);
                bin.enforce_spare_thread_policy(coming_soon);
                let atomic_waker = AtomicWaker::new();
                atomic_waker.register(cx.waker());
                let shared_state = Arc::new(SharedSet{children_remaining: AtomicSpawned::new(coming_soon.try_into().unwrap()), waker: atomic_waker});
                let children = tasks.into_iter().map(|f| {
                    //first, we need to box the future.  It might be boxed already, but in a lot of cases
                    //we have a consistent wrapping type we can use here, which is not necessarily boxed.
                    let boxed_child = Box::pin(f);
                    //so the idea here is we're erasing 'a to 'static.
                    //we can do this because,
                    //1. Future is valid for 'a
                    //2. We are valid for 'a
                    //3. We  will do something safe on drop.
                    let i_think_i_am = boxed_child as Pin<Box<dyn Future<Output=()> + Send + 'a>>;
                    let i_now_become: Pin<Box<dyn Future<Output=()> + Send + 'static>> = unsafe { std::mem::transmute(i_think_i_am) };
                    let child = Child {
                        shared: shared_state.clone(),
                        inner: i_now_become,
                        done: false,
                    };
                    Box::pin(child)
                });
                for item in children {
                    bin.spawn_without_hint(item);
                }
                match poll_thunk(&shared_state) {
                    Poll::Ready(_) => {
                        self.state = State::Done;
                        self.story.log("Set done immediately".to_string());
                        Poll::Ready(())
                    }
                    Poll::Pending => {
                        self.state = State::Spawned(shared_state);
                        Poll::Pending
                    }
                }
            }
            State::Spawned(state) => {
                match poll_thunk(&state) {
                    Poll::Ready(_) => {
                        self.story.log("Set done".to_string());
                        self.state = State::Done;
                        Poll::Ready(())
                    }
                    Poll::Pending => {
                        //register new? waker
                        state.waker.register(cx.waker());
                        //once we register waker, we need to check again to avoid race condition
                        //probably not ready, but let's make extra sure
                        match poll_thunk(&state) {
                            Poll::Ready(_) => {
                                self.story.log("Set done".to_string());
                                self.state = State::Done;
                                Poll::Ready(())
                            }
                            Poll::Pending => {
                                //give up
                                self.state = State::Spawned(state);
                                Poll::Pending
                            }
                        }
                    }
                }
            }
            State::Invalid => {unreachable!()}
            State::Done => {panic!("Already done!")}
        }
    }
}
pub trait Guard: std::future::Future {

}
impl<'a, F,V: Unpin + IntoIterator<Item=F>> Guard for InternalGuard<V> where Self: 'a, F: Future<Output=()> + Send {}

/**
Build a set from an iterator of tasks.

The tasks can access local state.  For this reason, we return a [Guard] of the same lifetime.  If tasks in the set are active
when leaving scope, the runtime will panic.
*/
pub fn set_scoped<'a,F,V: IntoIterator<Item=F> + Unpin + 'a>(priority: priority::Priority, tasks: V) -> impl Guard + 'a where F: Future<Output=()> + Send {
    InternalGuard {
        state: State::NotSpawned(tasks),
        priority,
        story: Story::new(),
    }
}
impl<V> Drop for InternalGuard<V> {
    fn drop(&mut self) {
        match &mut self.state {
            State::NotSpawned(_) | State::Done => {
                /* this is fine*/
            }
            State::Spawned(..) => {
                panic!("Can't drop a guard while tasks are active");
            }
            State::Invalid => {
                unreachable!();
            }
        }
    }
}
pub struct SimpleGuard<'a,F> {
    priority: Priority,
    shared: Arc<SharedSet>,
    data: PhantomData<&'a ()>,
    jobs: u32,
    spawned: bool,
    job_creator: &'a F,
}
/**
Build a set from an iterator of tasks.

This variant handles 'simple' tasks, eg. closures.
The tasks can access local state.  For this reason, we return a [Guard] of the same lifetime.  If tasks in the set are active
when leaving scope, the runtime will panic.
 */
pub fn set_simple_scoped<'a, F: Fn(u32) + Send + Sync>(priority: priority::Priority, jobs: u32, job_creator: &'a F) -> SimpleGuard<'a,F> {
    let shared = Arc::new(SharedSet {
        children_remaining: AtomicSpawned::new(jobs),
        waker: AtomicWaker::new()
    });
    SimpleGuard {
        job_creator,
        shared,
        jobs,
        priority,
        data: PhantomData,
        spawned: false,
    }
}
impl<F> Drop for SimpleGuard<'_,F> {
    fn drop(&mut self) {
        if self.shared.children_remaining.load(Ordering::Relaxed) != 0 {
            panic!()
        }
    }
}
impl<'a,F: Fn(u32) + Send + Sync> Future for SimpleGuard<'a,F> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut_self = self.get_mut();
        if !mut_self.spawned {
            let executor = Executor::global().bin_for(mut_self.priority);
            executor.enforce_spare_thread_policy(mut_self.jobs.try_into().unwrap());
            for job in 0..mut_self.jobs {
                let shared = mut_self.shared.clone();
                let move_job_creator = mut_self.job_creator;
                let new_job = Box::new(move || {
                    (move_job_creator)(job);
                    let item = shared.children_remaining.fetch_sub(1, Ordering::Relaxed);
                    if item == 1 {
                        shared.waker.wake();
                    }
                });
                //so the idea here is we're erasing 'a to 'static.
                //we can do this because,
                //1. Future is valid for 'a
                //2. We are valid for 'a
                //3. We  will do something safe on drop.
                let i_think_i_am = new_job as Box<dyn FnOnce() + Send + 'a>;
                let i_now_become: Box<dyn FnOnce() + Send + 'static> = unsafe { std::mem::transmute(i_think_i_am) };
                executor.spawn_simple_without_hint(i_now_become);
            }
            mut_self.spawned = true;
        }

        //doing this upfront seems a bit better?
        mut_self.shared.waker.register(cx.waker());
        if mut_self.shared.children_remaining.load(Ordering::Relaxed) == 0 {
            Poll::Ready(())
        }
        else {
            Poll::Pending
        }
    }
}

#[cfg(test)] mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use crate::set::{set_scoped, set_simple_scoped};
    use kiruna::test::{test_await,sparse_await};

    #[test] fn test_set() {
        let mut v = Vec::new();
        let stack_var = 5;
        v.push(Box::pin(async {
            let _a = &stack_var;
            //println!("{a}")
        } ) as Pin<Box<dyn Future<Output=()> + Send>>);

        for _ in 0..10_000 {
            v.push(Box::pin(async {
                let _a = &stack_var;
                //println!("another future {a}");
            }));
        }

        let guard = set_scoped(priority::Priority::Testing, v);
        test_await(guard, std::time::Duration::from_secs(2));
    }

    #[test] fn test_simple() {
        let stack_var = 5;
        let all_jobs = |arg| {
            let _a = &stack_var;
        };
        let guard = set_simple_scoped(priority::Priority::Testing, 10_000, &all_jobs);
        test_await(guard, std::time::Duration::from_secs(2));

    }

    #[test] fn test_sparse() {
        let mut v = Vec::new();
        v.push(Box::pin(async {
            println!("hello!");
        } ) as Pin<Box<dyn Future<Output=()> + Send>>);
        let guard = set_scoped(priority::Priority::Testing, v);
        sparse_await(guard, std::time::Duration::from_secs(2));
    }
}