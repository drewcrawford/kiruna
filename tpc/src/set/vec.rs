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


pub enum Strategy {
    /**
    Tries to schedule the number of jobs specified.  This is not generally recommended but is a useful option for profiling.

    The actual number of jobs will be the smaller of
    * the number specified
    * the length of the array
    * Some small adjustment to make jobs approximately evenly-sized; this may be one more or less job than requested.  This adjustment
     will be consistent across benchmarks. */
    Jobs(u32),
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

    1.  One microbenchmark I did suggests values of 10 work well on my system.
    2.  Apple's GCD documentation gives the following advice in an equivalent situation"
        > Higher iteration values give the system the ability to balance more efficiently across multiple cores. To get the maximum benefit of this function, configure the number of iterations to be at least three times the number of available cores."
    3. The upper bound is the size of the array.  I have designed tpc to be effective even at stupidly-high job counts, so you might be surprised at how well this works.  That said, there is a very small amount of overhead for scheduling jobs.

    */
    JobsPerCore(u32),
}

/**
This is a sort of proto-`VecBuilder` without any knowledge of types, data, or how to build them.
This is primarily used to extract the planner logic so we can unit test it more easily.
*/
#[derive(PartialEq,Debug)]
struct ChildPlan {
    ///the first index we will write to
    base_offset: usize,
    ///past the last index we will write to
    len: usize
}
#[derive(Clone)]
struct ChildPlanner {
    i: u32,
    k: usize,
    m: usize,
    target_jobs: u32,
}
impl ChildPlanner {
    fn new(strategy: Strategy, len: usize) -> Self {
        let target_jobs = {
            let proposed = match strategy {
                Strategy::Jobs(jobs) => {jobs}
                Strategy::JobsPerCore(jobs) => {jobs * GlobalState::global().physical_cpus as u32}
            };
            if proposed as usize >= len as usize {
                len as u32
            }
            else {
                proposed
            }
        };
        let k = len / target_jobs as usize;
        let m = len % target_jobs as usize;

        Self {
           k,m,
            i: 0,
        target_jobs,
        }
    }
    fn len(&self) -> u32 {
        self.target_jobs
    }
}
impl Iterator for ChildPlanner {
    type Item = ChildPlan;

    fn next(&mut self) -> Option<Self::Item> {
        if self.i == self.target_jobs {
            None
        }
        else {
            let i_as_usize = self.i as usize;
            let start = i_as_usize * self.k + i_as_usize.min(self.m);
            let i_plus_one = (self.i+1) as usize;
            let end = i_plus_one * self.k + i_plus_one.min(self.m);
            self.i += 1;
            Some(ChildPlan {
                base_offset: start,
                len: end - start
            })
        }
        
    }
}
struct Info<O> {
    base_ptr: Smuggle<O>,
    base: usize,
    len: usize,
    story: Story,
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
pub async fn set_sync<F,O>(priority: priority::Priority, len: usize, strategy: Strategy, f: F) -> Vec<O> where F:Fn(usize) -> O + Sync + Unpin + Send {
    let mut output = Vec::<O>::with_capacity(len);
    let raw_ptr = output.as_mut_ptr();

    let child_planner = ChildPlanner::new(strategy, len);
    let fut_len = child_planner.len();
    let mut jobs = Vec::with_capacity(fut_len.try_into().unwrap());
    for plan in child_planner {
        let base_ptr = unsafe{
            //hopefully our plan is correct
            raw_ptr.add(plan.base_offset)
        };
        jobs.push(Info {
            base_ptr: Smuggle(base_ptr),
            base: plan.base_offset,
            len: plan.len,
            story: Story::new()
        });
    }
    let job_creator = |index: u32| {
        let index_usize: usize = index.try_into().unwrap();
        let item: &Info<_> = &jobs[index_usize];
        let smuggled = &item.base_ptr;
        let mut write_ptr = smuggled.0;
        let mut slot = item.base;
        for _ in 0..item.len {
            unsafe {
                let val = (f)(slot);
                *write_ptr = val;
                write_ptr = write_ptr.add(1);
                slot += 1;
            }
        }
        let base = item.base;
        item.story.log(format!("Built from {base} to {slot}"));
    };
    //println!("launching {fut_len} tasks");
    let story = Story::new();
    story.log(format!("vec set await of length {fut_len}"));
    super::set_simple_scoped(priority, jobs.len().try_into().unwrap(), &job_creator ).await;
    story.log("vec set complete".to_string());
    unsafe{output.set_len(len)};
    output
}

#[cfg(test)] mod tests {
    use crate::set::vec::{ChildPlan, ChildPlanner, set_sync, Strategy};
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
    #[test] fn large_memory_write() {
        let test_len = 384*384;
        let big_fut = set_sync(priority::Priority::Testing, test_len, Strategy::JobsPerCore(10_000), |idx| {
            idx as f32
        });
        let my_vec = test_await(big_fut, std::time::Duration::new(100, 0));
        assert_eq!(my_vec.len(), test_len);
        for (i,item) in my_vec.iter().enumerate() {
            assert_eq!(i as f32,*item);
        }
    }
    #[test] fn echos_hill_texture() {
        let mut planner = ChildPlanner::new(Strategy::Jobs(1), 384*384);
        assert_eq!(planner.clone().count(), 1);
        assert_eq!(planner.next().unwrap(), ChildPlan{base_offset: 0, len: 384*384});

        planner = ChildPlanner::new(Strategy::Jobs(2), 384*384);
        assert_eq!(planner.clone().count(), 2);
        assert_eq!(planner.next().unwrap(), ChildPlan{base_offset: 0, len: 384*384/2});
        assert_eq!(planner.next().unwrap(), ChildPlan{base_offset: 384*384/2, len: 384*384/2});

        planner = ChildPlanner::new(Strategy::Jobs(3), 384*384);
        assert_eq!(planner.clone().count(), 3);
        assert_eq!(planner.next().unwrap(), ChildPlan{base_offset: 0, len: 384*384/3});

        planner = ChildPlanner::new(Strategy::Jobs(5), 384*384);
        assert_eq!(planner.clone().count(), 5);
        let mut offset = 0;
        for plan in planner {
            assert_eq!(plan.base_offset,offset);
            offset += plan.len;
            assert!(plan.len == 29491 || plan.len == 29492);
        }

        planner = ChildPlanner::new(Strategy::Jobs(20000), 384*384);
        assert_eq!(planner.clone().count(), 20000);
        offset = 0;
        for plan in planner {
            assert_eq!(plan.base_offset,offset);
            offset += plan.len;
            assert!(plan.len == 7 || plan.len == 8);
        }

        planner = ChildPlanner::new(Strategy::Jobs(1_000_000_000), 384*384);
        assert_eq!(planner.clone().count(), 384*384);
        offset = 0;
        for plan in planner {
            assert_eq!(plan.base_offset,offset);
            offset += plan.len;
            assert_eq!(plan.len, 1);
        }
    }
}