/*!
A set is a collection of tasks that execute in parallel.

The primary way to interact with a set is through a [Guard].
The [Guard] acts as a future that polls the set as a whole;
when the Guard becomes [poll::Ready], the entire set has completed.

# Design

The rationale for this design may not be completely obvious.  For example, doesn't [kiruna::join] do something pretty similar?

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
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::{Context, Poll};
use crate::{Executor};

type AtomicSpawned = AtomicU32;
type Spawned = u32;
struct SharedSet {
    children_done: AtomicSpawned,
}

struct Child<F> {
    shared: Arc<SharedSet>,
    inner:F
}
impl<F: Future<Output=()>> Future for Child<F> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let tmp_shared = /* unsafely borrow - does not overlap with projection */ unsafe{&*(&self.shared as *const Arc<SharedSet>)};
        //unsafely project our argument - does not overlap with borrow
        let child = unsafe{self.map_unchecked_mut(|a| &mut a.inner)};
        match child.poll(cx) {
            Poll::Ready(_) => {
                tmp_shared.children_done.fetch_add(1, Ordering::Relaxed);
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
    Spawned(Arc<SharedSet>,Spawned),
    Done,
    Invalid
}

struct InternalGuard<V> {
    state: State<V>,
    priority: priority::Priority,
}

impl<'a, F,V: IntoIterator<Item=F> + Unpin> Future for InternalGuard<V> where Self: 'a, F: Future<Output=()> + Send + 'a {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        fn poll_thunk(set: &Arc<SharedSet>, spawned: Spawned) -> Poll<()> {
            let load = set.children_done.load(Ordering::Relaxed);
            if load == spawned {
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
                let shared_state = Arc::new(SharedSet{children_done: AtomicSpawned::new(0)});
                let iter = tasks.into_iter();
                let bin = Executor::global().bin_for(self.priority);
                let children = iter.map(|f| {
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
                        inner: i_now_become
                    };
                    Box::pin(child)
                });
                let (coming_soon, _) = children.size_hint();
                bin.enforce_spare_thread_policy(coming_soon);
                let mut spawned = 0;
                for item in children {
                    bin.spawn_without_hint(item);
                    spawned += 1;
                }
                match poll_thunk(&shared_state,spawned) {
                    Poll::Ready(_) => {
                        self.state = State::Done;
                        Poll::Ready(())
                    }
                    Poll::Pending => {
                        self.state = State::Spawned(shared_state, spawned);
                        Poll::Pending
                    }
                }
            }
            State::Spawned(state,spawned) => {
                match poll_thunk(&state, spawned) {
                    Poll::Ready(_) => {
                        self.state = State::Done;
                        Poll::Ready(())
                    }
                    Poll::Pending => {
                        self.state = State::Spawned(state, spawned);
                        Poll::Pending
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
        priority
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

#[cfg(test)] mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use crate::set::set_scoped;
    use kiruna::test::test_await;

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
        test_await(guard, std::time::Duration::from_millis(1000));
    }
}