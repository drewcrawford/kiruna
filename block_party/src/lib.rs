/*!
A microthreadpool for blocking IO.

block_party solves a common problem in async Rust, which is:
1.  You have to call a blocking function
2.  You can't block; you need to suspend/await instead.
3.  So you need to write all a future with all its boilerplate that you don't remember.  Also you need to spawn a thread to handle the wakeups.  But maybe not 50 threads if you do this 50 times back to back?  That seems bad?

This is where block_party comes in, it makes your "just block as a future" problem easy and efficient.  To solve this problem, you simply need to implement the [Task] trait, and then call [Future::new()] on your type.  To do this,
you will need to implement the following small functionalities:

1.  An value of the [Task] to wait on.  This may be a semaphore, fence, IO operation, etc.
2.  To create an extra task, not related to your problem, but for block_party's use.  This task is called the [Sidechannel].  It does not need to be the same type as [Task], but if not, you must be able to wait on both types.
3.  Block on an array of tasks ([Task::wait_any], including the [Sidechannel], waking when any one of the tasks wake.  Return the output of the task.
4.  Implement a method for block_party to [Sidechannel::wake] the [Sidechannel].
5.  A [Pool] in which to scope tasks.  You will only be asked to wait on tasks from a single pool.  Usually you want this to be a global static, but if there are some restrictions on which [Task]s can be waited together, such as
they belong to a shared resource, you can scope them here.

With those steps, block_party will assemble the rest of the problem for you.
*/
mod pool;
mod future;

pub use future::Future;
pub use crate::pool::Pool;


pub enum WakeResult<Output> {
    ///We woke with one of the tasks specified.  First item is the index into the array of Tasks, second is the output of that task.
    Task(usize,Output),
    ///Woke due to the sidechannel
    Sidechannel,
}
/**
A Task for block_party's internal use.  You must be able to wait on this in addition to your other tasks.
*/
pub trait Sidechannel: Sync + Send {
    /**
    Implementations for this method should cause any active wait_any calls to return [WakeResult::Sidechannel].
    */
    fn wake(&self);

    /**
    Implement this to indicate that your sidechannel may only be waited one time.
     If you return `true`, the sidechannel will be recreated after each call to `wake`.
*/
    fn one_wait_only() -> bool { false }
}

/**
Implement this type to describe your problem.
*/
pub trait Task: Sized + 'static + Unpin + Send {
    /**
    The output we will produce from this Task.  the result of our file IO, etc.  May be unit.
    */
    type Output: Send;
    ///A special task for block_party's use.  You must be able to wait on this type in addition to a collection of Tasks.
    type Sidechannel: Sidechannel;
    /*
    An arbitrary type that is shared by the entire [Pool].  Can be unit if you don't need it.
     */
    type Pool: Send + Sync;
    ///Create a new SideChannel.
    fn make_side_channel(pool: &Self::Pool) -> Self::Sidechannel;
    /**
    Block until any one of the tasks or side_channel has woken.

    Return information about the woken task.
    */
    fn wait_any(pool: &Self::Pool, tasks: &[Self], side_channel: &Self::Sidechannel) -> WakeResult<Self::Output>;
}




#[cfg(test)] mod tests {
    use std::pin::Pin;
    use std::task::Poll;
    use std::time::Duration;
    use crossbeam_channel::{Receiver, Sender};
    use rand::Rng;
    use priority::Priority;
    use crate::{Future, WakeResult};
    use crate::Pool;
    use std::sync::atomic::{AtomicBool, Ordering};
    /* Test equipment */
    fn test_channel() -> (Sender<u8>,Receiver<u8>) {
        crossbeam_channel::bounded(10)
    }
    #[derive(Debug)]
    struct MyTask {
        id: u8,
    }
    #[derive(Debug)]
    struct Sidechannel{
        sender: Sender<u8>
    }
    impl super::Sidechannel for Sidechannel {
        fn wake(&self) {
           self.sender.send(u8::MAX).unwrap();
        }
    }
    impl super::Task for MyTask {
        type Pool = (Sender<u8>,Receiver<u8>);
        type Output = u8;
        type Sidechannel = Sidechannel;

        fn make_side_channel(pool: &(Sender<u8>,Receiver<u8>)) -> Self::Sidechannel {
            Sidechannel{ sender: pool.0.clone() }
        }

        fn wait_any(pool: &(Sender<u8>,Receiver<u8>), tasks: &[Self], _side_channel: &Self::Sidechannel) -> WakeResult<Self::Output> {
            assert!(!tasks.iter().any(|t| t.id == u8::MAX));
            println!("waiting on tasks {:?}",tasks);
            let a_channel = &pool.1;
            let result = a_channel.recv().unwrap();
            println!("awoke with result {result}");
            if result == u8::MAX {
                WakeResult::Sidechannel
            }
            else {
                let task = tasks.iter().enumerate().find(|(_,task)| task.id == result).unwrap();
                WakeResult::Task(task.0, 23)
            }
        }
    }
    #[test] fn test_future() {
        let test_channel = test_channel();
        let keep_sender = test_channel.0.clone();
        let pool = Pool::new_with(test_channel);
        let task = MyTask {
            id: 0,
        };
        let future = crate::future::Future::new(&pool,task, Priority::Testing);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            keep_sender.send(0).unwrap();
        });
        let r = kiruna::test::test_await(future, std::time::Duration::from_secs(1));
        assert!(r == 23);

    }

    #[test] fn test_two_futures() {
        let test_channel = test_channel();
        let keep_sender = test_channel.0.clone();
        let pool = Pool::new_with(test_channel);
        let task = MyTask {
            id: 1
        };
        let mut future = crate::future::Future::new(&pool,task, Priority::Testing);
        let mut future = unsafe{Pin::new_unchecked(&mut future)};
        let task2 = MyTask {
            id: 2,
        };
        let mut future2 = crate::future::Future::new(&pool,task2, Priority::Testing);
        let mut future2 = unsafe{Pin::new_unchecked(&mut future2)};

        let r = kiruna::test::test_poll_pin(&mut future);
        assert_eq!(r, Poll::Pending);
        let r2 = kiruna::test::test_poll_pin(&mut future2);
        assert_eq!(r2, Poll::Pending);

        std::thread::sleep(Duration::from_millis(100)); //wait for the poll to actually take effect.
        keep_sender.send(2).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let r = kiruna::test::test_poll_pin(&mut future);
        assert_eq!(r, Poll::Pending);

        let r2 = kiruna::test::test_poll_pin(&mut future2);
        assert_eq!(r2, Poll::Ready(23));
    }

    #[test] fn sparse() {
        let test_channel = test_channel();
        let keep_sender = test_channel.0.clone();
        let pool = Pool::new_with(test_channel);
        let task = MyTask {
            id: 3
        };
        let future = crate::future::Future::new(&pool,task, Priority::Testing);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            keep_sender.send(3).unwrap();
        });
        kiruna::test::sparse_await(future, Duration::from_secs(5));
    }

    #[test] fn arrive_late() {
        let test_channel = test_channel();
        let keep_sender = test_channel.0.clone();
        let pool = Pool::new_with(test_channel);
        for i in 0..5 {
            println!("arrive_late iteration {i}");
            let task = MyTask {
                id: 3
            };

            //poll first future
            let mut future = crate::future::Future::new(&pool,task, Priority::Testing);
            let mut future = unsafe{Pin::new_unchecked(&mut future)};
            assert_eq!(kiruna::test::test_poll_pin(&mut future), Poll::Pending);

            //poll second future
            let task2 = MyTask {
                id: 4
            };
            let mut future2 = crate::future::Future::new(&pool, task2, Priority::Testing);
            let mut future2 = unsafe{Pin::new_unchecked(&mut future2)};
            assert_eq!(kiruna::test::test_poll_pin(&mut future2),Poll::Pending);

            //complete 2nd future
            keep_sender.send(4).unwrap();
            kiruna::test::test_await_pin(&mut future2, Duration::from_secs(5));
            //complete 1st future
            keep_sender.send(3).unwrap();
            kiruna::test::test_await_pin(&mut future, Duration::from_secs(5));
        }

    }
    #[test] fn drop_check() {
        struct DropTask {
            complete: AtomicBool
        }
        impl Drop for DropTask {
            fn drop(&mut self) {
                assert!(self.complete.load(Ordering::Relaxed));
            }
        }
        impl crate::Sidechannel for DropTask {
            fn wake(&self) {
                /* no effect; we will wake the sidechannel randomly */
            }
        }
        impl crate::Task for DropTask {
            type Output = ();
            type Sidechannel = DropTask;
            type Pool = ();

            fn make_side_channel(_pool: &Self::Pool) -> Self::Sidechannel {
                DropTask { complete: AtomicBool::new(true) /* no consequences for dropping the sidechannel. */ }
            }

            fn wait_any(_pool: &Self::Pool, tasks: &[Self], _side_channel: &Self::Sidechannel) -> WakeResult<Self::Output> {
                // println!("wait {} tasks",tasks.len());
                std::thread::sleep(Duration::from_millis(1));
                if rand::thread_rng().gen_bool(0.5) {
                    WakeResult::Sidechannel
                }
                else {
                    let rand = rand::thread_rng().gen_range(0..tasks.len());
                    tasks[rand].complete.store(true, Ordering::Relaxed);
                    WakeResult::Task(rand, ())
                }
            }
        }
        let mut test_executor = kiruna::sync::Executor::new();
        let pool = Pool::new();
        for _ in 0..100 {
            test_executor.spawn(Future::new(&pool, DropTask { complete: AtomicBool::new(false)}, Priority::Testing));
            test_executor.do_some();
        }
        test_executor.drain();
    }
}
