/*!
Sets that build vecs of values.
*/

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use crate::global::GlobalState;
use crate::stories::Story;

struct Smuggle<O>(*mut O);
impl<O> Clone for Smuggle<O> {
    fn clone(&self) -> Self {
        Self(self.0)
    }
}
unsafe impl<O> Send for Smuggle<O> {}
unsafe impl<O> Sync for Smuggle<O> {}

struct VecBuilder<O,F> {
    base_ptr: Smuggle<O>,
    base: usize,
    len: usize,
    generator: F,
}
impl<O,F> Future for VecBuilder<O,F> where F: Fn(usize) -> O {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut write_ptr = self.base_ptr.0;
        let mut slot = self.base;
        for _ in 0..self.len {
            unsafe {
                let val = (self.generator)(slot);
                *write_ptr = val;
                write_ptr = write_ptr.add(1);
                slot += 1;
            }
        }
        Poll::Ready(())
    }
}

pub enum Strategy {
    /**
    Tries to schedule the number of jobs specified.  This is not generally recommended but is a useful option for profiling.

    The actual number of jobs will be the smaller of
    * the number specified
    * the length of the array
    * Some small adjustment to make jobs approximately evenly-sized; this may be one more or less job than requested.  This adjustment
     will be consistent across benchmarks. */
    Jobs(usize),
    /**
    Tries to schedule the number of jobs specified multiplied by the number of cores on the system.  This lets you scale the
    parallelism and threading based on the system's ability to take advantage of it.

    This does not guarantee the jobs will run on all cores or that each core will perform exactly or even roughly the jobs specified here.
    It simply makes the granularity available to tpc where that could happen.

    # How to pick a value

    Benchmark, obviously.  But let me sketch a mental model.

    Obviously, if you schedule fewer jobs than cores, some cores will be idle so that's not great.  So you pass in [Strategy::JobsPerCore(1)] and have full
    parallelism, right?

    Not exactly.  Due to other work in our out of the program those cores won't start simultaneously.  The jobs won't be 100% evenly-sized either, and on asymmetric hardware
    performance will be different across core types.  So what happens is there is some "last job" running on one core, it is effectively
    single-threaded until it finishes.

    So, you want to keep job sizes small enough that the "last job" will be quick.  That means passing a value substantially greater than 1 in here.
    How much greater?

    1.  One microbenchmark I did suggests values of 28 and 56 work well on my system.
    2.  Apple's GCD documentation gives the following advice in an equivalent situation"
        > Higher iteration values give the system the ability to balance more efficiently across multiple cores. To get the maximum benefit of this function, configure the number of iterations to be at least three times the number of available cores."
    3. The upper bound is the size of the array.  I have designed tpc to be effective even at stupidly-high job counts, so you might be surprised at how well this works.  That said, there is a very small amount of overhead for scheduling jobs.

    */
    JobsPerCore(usize),
}

/**
Builds a [Vec] by executing parallel jobs.

* priority: The priority for spawning jobs.
* len: The length of the output [Vec].
* strategy: The strategy for dividing work into jobs.  See [Strategy].
* f: A function which builds one independent element of the output.  The function will be called in a loop in each job.

Returns a future.  When polled, jobs will start.  The future will produce the built Vec.  The future can be polled on any async runtime.
The underlying jobs will execute on tpc.
*/
pub async fn set_sync<F,O>(priority: priority::Priority, len: usize, strategy: Strategy, f: F) -> Vec<O> where F:Fn(usize) -> O + Sync {
    let mut output = Vec::<O>::with_capacity(len);
    let target_tasks = match strategy {
        Strategy::Jobs(jobs) => {jobs}
        Strategy::JobsPerCore(jobs) => {
            jobs * GlobalState::global().physical_cpus as usize
        }
    };
    let raw_ptr = output.as_mut_ptr();
    let divide_by = (target_tasks - 1).max(1);
    let each_task_up_to_not_including = (len / divide_by as usize).max(1);

    let mut futures = Vec::with_capacity(target_tasks as usize);
    for task in 0..target_tasks {
        let start_offset = task as usize * each_task_up_to_not_including as usize;
        //clamp this value to the end.
        let end_offset = (start_offset + each_task_up_to_not_including as usize).min(len);
        if start_offset > end_offset {
            break; //don't emit any more futures
        }
        let fut_len = end_offset - start_offset;
        if fut_len > 0 {
            let fut = VecBuilder {
                base_ptr: Smuggle(unsafe{raw_ptr.add(start_offset)}),
                base: start_offset,
                len: end_offset - start_offset,
                generator: &f
            };
            futures.push(fut);
        }
    }
    let fut_len = futures.len();
    //println!("launching {fut_len} tasks");
    let story = Story::new();
    story.log(format!("vec set await of length {fut_len}"));
    super::set_scoped(priority, futures).await;
    story.log("vec set complete".to_string());
    unsafe{output.set_len(len)};
    output
}

#[cfg(test)] mod tests {
    use crate::set::vec::{set_sync, Strategy};
    use kiruna::test::test_await;
    #[test] fn build_vec() {
        let test_len = 1_000;
        let big_fut = set_sync(priority::Priority::Testing, test_len, Strategy::Jobs(100),|idx| {
            idx
        });
        let my_vec = test_await(big_fut, std::time::Duration::new(100, 0));
        assert_eq!(my_vec.len(), test_len);
        for (i,item) in my_vec.iter().enumerate() {
            assert_eq!(i,*item);
        }
    }
    #[test] fn compute_check() {
        let test_len = 5_000;
        let big_fut = set_sync(priority::Priority::Testing, test_len, Strategy::Jobs(100), |idx| {
            let mut val = idx;
            for _ in 0..1_00_000 {
                val ^= idx;
            }
            val
        });
        let my_vec = test_await(big_fut, std::time::Duration::new(100, 0));
        assert_eq!(my_vec.len(), test_len);
    }
    #[test] fn build_many_jobs() {
        let test_len = 5_000;
        let big_fut = set_sync(priority::Priority::Testing, test_len, Strategy::Jobs(10_000), |idx| {
            idx
        });
        let my_vec = test_await(big_fut, std::time::Duration::new(100, 0));
        assert_eq!(my_vec.len(), test_len);
        for (i,item) in my_vec.iter().enumerate() {
            assert_eq!(i,*item);
        }
    }
    #[test] fn build_one_job() {
        let test_len = 5_000;
        let big_fut = set_sync(priority::Priority::Testing, test_len, Strategy::Jobs(1), |idx| {
            idx
        });
        let my_vec = test_await(big_fut, std::time::Duration::new(100, 0));
        assert_eq!(my_vec.len(), test_len);
        for (i,item) in my_vec.iter().enumerate() {
            assert_eq!(i,*item);
        }
    }
}