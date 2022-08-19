use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::sync::{Arc};
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use atomic_waker::AtomicWaker;
use priority::Priority;
use crate::pool::{Pool, SendSideInner, WorkerSideInfo};

struct MailboxShared<Output> {
    has_output: AtomicBool,
    /*
    1.  Only the sender writes, and it has an exclusive pointer.
    2.  It only writes once, as verified by itself
    3.  We only read once, as verified by the receiver.
     */
    output: UnsafeCell<MaybeUninit<Output>>,
    waker: AtomicWaker,
}

pub(crate) struct MailboxSender<Output> {
    sent: bool,
    shared: Arc<MailboxShared<Output>>,
}
//safety: we will synchronize the mailbox manually
unsafe impl<Output> Send for MailboxSender<Output> {}
struct MailboxReceiver<Output> {
    shared: Arc<MailboxShared<Output>>,
    read: bool,
}
fn mailbox<Output>() -> (MailboxSender<Output>,MailboxReceiver<Output>) {
    let shared = Arc::new(MailboxShared {
        has_output: AtomicBool::new(false),
        output: UnsafeCell::new(MaybeUninit::uninit()),
        waker:AtomicWaker::new()
    });
    (
        MailboxSender {
            sent: false,
            shared: shared.clone(),
        },
        MailboxReceiver {
            shared: shared.clone(),
            read: false,
        }
    )
}
/*Hopefully we transferred Output correctly. */
unsafe impl<Output: Send> Sync for MailboxSender<Output> {}
impl<Output> MailboxSender<Output> {

    pub(crate) fn send_mail(&mut self, output: Output) {
        assert!(!self.sent);
        self.sent = true;

        //we know there is no writer because we have exlcusive access to the MailboxSender
        //we know there is no other reader because they are gated on the atomic.
        unsafe{*self.shared.output.get() = MaybeUninit::new(output)};
        let old_value = self.shared.has_output.swap(true, Ordering::Release); //ensure our write happens-before.
        //just double-check to be sure
        assert!(!old_value);
        self.shared.waker.wake();
    }
}
impl<Output> MailboxReceiver<Output> {
    pub(crate) fn recv_mail(&mut self) -> Option<Output> {
        //ensure our atomic happens-before reading the underlying memory
        if self.shared.has_output.load(Ordering::Acquire) {
            assert!(!self.read);
            self.read = true;
            //1. we know object was initialized due to has_output
            //2.  we know object is ready once due to self.read
            Some(unsafe{(&*self.shared.output.get()).assume_init_read()})
        }
        else {
            None
        }
    }
}
struct TaskInfo<Task: crate::Task> {
    task: Task,
    mailbox_sender: MailboxSender<Task::Output>,
}
pub struct Future<Task: crate::Task> {
    task: Option<TaskInfo<Task>>,
    mailbox: MailboxReceiver<Task::Output>,
    priority: Priority,
    pool_inner: Arc<SendSideInner<Task>>,
}
impl<Task: crate::Task> Future<Task> {
    /**
    Creates a new future from the specified task.
     */
    pub fn new(pool: &Pool<Task>, task: Task, priority: Priority) -> Self {
        let (mailbox_sender,mailbox_receiver) = mailbox();
        let task_info = TaskInfo {
            task: task,
            mailbox_sender
        };

        Self{
            priority,
            task: Some(task_info),

            mailbox: mailbox_receiver,
            pool_inner: pool.send_side_inner.clone(),
    }}
}

impl<Task: crate::Task> std::future::Future for Future<Task> {
    type Output = Task::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        fn poll_thunk<Task: crate::Task>(unpin: &mut Future<Task>) -> Poll<Task::Output> {
            unpin.mailbox.recv_mail().map_or(Poll::Pending, |f| Poll::Ready(f))
        }
        let priority = self.priority;
        let unpin = self.get_mut();
        match unpin.task.take() {
            None => {
                match poll_thunk(unpin) {
                    Poll::Ready(result) => {Poll::Ready(result)}
                    Poll::Pending => {
                        unpin.mailbox.shared.waker.register(cx.waker());
                        poll_thunk(unpin) //try one more time
                    }
                }
            }
            Some(task) => {
                unpin.mailbox.shared.waker.register(cx.waker());
                let info = WorkerSideInfo{
                    task: task.task,
                    mailbox: task.mailbox_sender,
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