use std::mem::MaybeUninit;
use std::pin::Pin;
use std::sync::{Arc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use atomic_waker::AtomicWaker;
use priority::Priority;
use crate::pool::{Pool, SendSideInner, WorkerSideInfo};

pub(crate) struct Mailbox<Output> {
    has_output: AtomicBool,
    //output has many special rules for access including gone, has_output field, etc.
    output: MaybeUninit<Output>,
    waker: AtomicWaker,
}
/*Hopefully we transferred Output correctly. */
unsafe impl<Output: Send> Sync for Mailbox<Output> {}
impl<Output> Mailbox<Output> {
    /**
    # Safety
     * there must be no other writers to this instance anywhere in the process
    * value must be written to exactly once

*/
    pub(crate) unsafe fn send_mail(&self, output: Output) {
        //terrible terrible idea
        let output_ptr = &self.output as *const _ as *mut Output;
        *output_ptr = output;
        self.has_output.store(true, Ordering::Release); //we need to synchronize with the read operation here
        self.waker.wake();
    }
}
pub struct Future<Task: crate::Task> {
    task: Option<Task>,
    mailbox: Arc<Mailbox<Task::Output>>,
    priority: Priority,
    gone: bool,
    pool_inner: Arc<SendSideInner<Task>>,
}
impl<Task: crate::Task> Future<Task> {
    /**
    Creates a new future from the specified task.
     */
    pub fn new(pool: &Pool<Task>, task: Task, priority: Priority) -> Self { Self{
        priority, task: Some(task),
        gone: false,
        mailbox: Arc::new(Mailbox{has_output: AtomicBool::new(false), output: MaybeUninit::uninit() , waker: AtomicWaker::new()}),
        pool_inner: pool.send_side_inner.clone(),

    }}
}

impl<Task: crate::Task> std::future::Future for Future<Task> {
    type Output = Task::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        fn poll_thunk<Task: crate::Task>(unpin: &mut Future<Task>) -> Poll<Task::Output> {
            if unpin.gone {
                panic!("Gone")
            }
            if unpin.mailbox.has_output.load(Ordering::Acquire) {
                let take = unsafe {
                    let take = unpin.mailbox.output.assume_init_read();
                    unpin.gone = true; //never allow us to do this again
                    take
                };

                Poll::Ready(take)
            }
            else {
                Poll::Pending
            }
        }
        let priority = self.priority;
        let unpin = self.get_mut();
        match unpin.task.take() {
            None => {
                match poll_thunk(unpin) {
                    Poll::Ready(result) => {Poll::Ready(result)}
                    Poll::Pending => {
                        unpin.mailbox.waker.register(cx.waker());
                        poll_thunk(unpin) //try one more time
                    }
                }
            }
            Some(task) => {
                unpin.mailbox.waker.register(cx.waker());
                let info = WorkerSideInfo{
                    task,
                    mailbox: unpin.mailbox.clone(),
                };
                match priority {
                    Priority::UserWaiting | Priority::Testing => {
                        unpin.pool_inner.launch_if_needed_user_waiting(info)

                    }
                    _ => todo!()
                }
                Poll::Pending
            }
        }
    }
}