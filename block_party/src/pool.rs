use std::fmt::{Debug, Formatter};
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicBool, compiler_fence, Ordering};
use std::time::Duration;
use crossbeam_channel::{Receiver, Sender};
use spawn::{MicroPriority, spawn_thread};
use crate::{Sidechannel, WakeResult};
use crate::future::MailboxSender;

pub(crate) struct PoolInner<Task: crate::Task> {
    receiver: Receiver<WorkerSideInfo<Task>>,
    thread_launched: AtomicBool,
    //todo: I suspect there is some way to do this with atomics but I'm not certain
    side_channel: RwLock<Task::Sidechannel>,
    pool_user: Task::Pool,
}
impl<Task: crate::Task> Debug for PoolInner<Task> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let receiver = &self.receiver;
        let thread_launched = &self.thread_launched;
        f.write_fmt(format_args!("<PoolInner{{receiver: {receiver:?} thread_launched: {thread_launched:?}, ..}}"))
    }
}

#[derive(Debug)]
pub(crate) struct SendSideInner<Task: crate::Task> {
    sender: Sender<WorkerSideInfo<Task>>,
    inner: Arc<PoolInner<Task>>,
}

/**
A scope for [crate::Task]s.

You will only be asked to wait on [crate::Task]s for one pool in a single call.  If there are no restrictions on how tasks can be intermixed for waiting,
you want to use a global pool for the best performance.  Place the [Pool] into a global static so that it is reused
across [crate::Future]s.

In some cases, [crate::Task]s are scoped to a particular resource, such as a hardware device, and [crate::Task]s from different devices
cannot be awaited together in a single call.  If so, you want to create one [Pool] per resource, and use the same pool instance
across all [crate::Task]s with the same resource.  Tasks will be segregated by [Pool] for waiting, so that you are only asked to wait
on one pool's tasks per call.  Maintaining multiple pools has minor performance overheads in some cases but it is necessary
to correctly implement some blocking APIs.
*/
#[derive(Debug)]
pub struct Pool<Task: crate::Task> {
    pub(crate) send_side_inner: Arc<SendSideInner<Task>>,
}

impl<Task: crate::Task> SendSideInner<Task> {
    pub(crate) fn launch_if_needed_user_waiting(&self, info: WorkerSideInfo<Task>) {
        self.sender.send(info).unwrap();
        let thread_launched = self.inner.thread_launched.swap(true, Ordering::Relaxed);
        if !thread_launched {

            compiler_fence(Ordering::Release); //ensure task was really sent
            let move_pool_inner = self.inner.clone();
            spawn_thread(priority::Priority::UserWaiting, MicroPriority::NEW, "kiruna block_party user_waiting", || {worker_fn_user_waiting::<Task>(move_pool_inner) });
        }
        else {
            compiler_fence(Ordering::Release); //ensure task was really sent
            self.inner.side_channel.read().unwrap().wake(); //tell thread about new task
        }
    }

}
impl<Task: crate::Task<Pool=()>> Pool<Task> {
    /**
    Creates a new pool.

    To use this method, [crate::Task::Pool] must be Unit.  If not, use [Self::new_with].
*/
    pub fn new() -> Self {
        Self::new_with(())
    }
}
impl<Task: crate::Task> Pool<Task> {
    /**
    Creates a new pool with the specified [crate::Task::Pool] value.  This value will be passed to various methods on the [crate::Task]
    */
    pub fn new_with(pool: Task::Pool) -> Self {
        let (sender,receiver) = crossbeam_channel::bounded(1);

        let inner = Arc::new(PoolInner {
            receiver: receiver,
            thread_launched: AtomicBool::new(false),
            side_channel: RwLock::new(Task::make_side_channel(&pool)),
            pool_user: pool
        });
        Pool {
            send_side_inner: Arc::new(SendSideInner{
                sender: sender,
                inner: inner,
            }),
        }
    }
}

/**
Performs one call to wait_one.  Updates tasks and sidechannel as needed. */
fn wait_any_one<Task: super::Task>(tasks: &mut Vec<Task>, mailboxes: &mut Vec<MailboxSender<Task::Output>>, pool_inner: &PoolInner<Task>) {
    let result = Task::wait_any(&pool_inner.pool_user,tasks.as_slice(), &pool_inner.side_channel.read().unwrap());
    match result {
        WakeResult::Task(idx, output) => {
            tasks.remove(idx);
            let mut mailbox = mailboxes.remove(idx);
            mailbox.send_mail(output);
            if Task::Sidechannel::one_wait_only() {
                //refresh the sidechannel
                let new_channel = Task::make_side_channel(&pool_inner.pool_user);
                *pool_inner.side_channel.write().unwrap() = new_channel; //write new channel
            }
        }
        WakeResult::Sidechannel => {
            //refresh the side_channel
            let new_channel = Task::make_side_channel(&pool_inner.pool_user);
            *pool_inner.side_channel.write().unwrap() = new_channel; //write new channel
        }
    }
}
fn read_greedy<Task: super::Task>(tasks: &mut Vec<Task>, mailboxes: &mut Vec<MailboxSender<Task::Output>>, receiver: &Receiver<WorkerSideInfo<Task>>) {
    while let Ok(info) = receiver.recv_timeout(Duration::ZERO) {
        tasks.push(info.task);
        mailboxes.push(info.mailbox);
    }
}
fn worker_fn_user_waiting<Task: super::Task>(pool_inner: Arc<PoolInner<Task>>) {
    let mut tasks = Vec::new();
    let mut mailboxes = Vec::new();
    read_greedy(&mut tasks, &mut mailboxes, &pool_inner.receiver);
    loop {
        if !tasks.is_empty() {
            read_greedy(&mut tasks, &mut mailboxes, &pool_inner.receiver);
            wait_any_one(&mut tasks, &mut mailboxes, &pool_inner);
        }
        else {
            //try a longer poll.  Avoid shutting down if possible
            //because in some workloads we will immediately relaunch with a new task
            //even though we don't have one at this exact moment.
            match pool_inner.receiver.recv_timeout(Duration::from_secs(1)) {
                Ok(info) => {
                    tasks.push(info.task);
                    mailboxes.push(info.mailbox);
                }
                Err(_) => {
                    break;
                }
            }
        }
    }
    //inform everyone we're shutting down
    pool_inner.thread_launched.store(false, Ordering::Relaxed);
}

pub(crate) struct WorkerSideInfo<Task: crate::Task> {
    pub(crate) task: Task,
   pub(crate) mailbox: MailboxSender<Task::Output>,
}


