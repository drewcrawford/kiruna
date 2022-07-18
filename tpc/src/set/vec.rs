/*!
Sets that build vecs of values.
*/

use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use crate::global::GlobalState;

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

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
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
    Jobs(usize),
}

pub async fn set_sync<F,O: Debug>(priority: priority::Priority, len: usize, strategy: Strategy, f: F) -> Vec<O> where F:Fn(usize) -> O + Sync {
    let mut output = Vec::<O>::with_capacity(len);
    let target_tasks = match strategy {
        Strategy::Jobs(jobs) => {jobs}
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
    super::set_scoped(priority, futures).await;
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
        let my_vec = test_await(big_fut, std::time::Duration::new(1, 0));
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
        let my_vec = test_await(big_fut, std::time::Duration::new(10, 0));
        assert_eq!(my_vec.len(), test_len);
    }
    #[test] fn build_many_jobs() {
        let test_len = 5_000;
        let big_fut = set_sync(priority::Priority::Testing, test_len, Strategy::Jobs(10_000), |idx| {
            idx
        });
        let my_vec = test_await(big_fut, std::time::Duration::new(10, 0));
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
        let my_vec = test_await(big_fut, std::time::Duration::new(10, 0));
        assert_eq!(my_vec.len(), test_len);
        for (i,item) in my_vec.iter().enumerate() {
            assert_eq!(i,*item);
        }
    }
}