use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicBool, compiler_fence, Ordering};
use std::time::Duration;
use crossbeam_channel::{Receiver, Sender};
use spawn::{MicroPriority, spawn_thread};
use crate::{Sidechannel, WakeResult};
use crate::future::Mailbox;

pub(crate) struct PoolInner<Task: crate::Task> {
    receiver: Receiver<WorkerSideInfo<Task>>,
    thread_launched: AtomicBool,
    //todo: I suspect there is some way to do this with atomics but I'm not certain
    side_channel: RwLock<Task::Sidechannel>,
    pool_user: Task::Pool,
}
pub(crate) struct SendSideInner<Task: crate::Task> {
    sender: Sender<WorkerSideInfo<Task>>,
    inner: Arc<PoolInner<Task>>,
}
use crate::Task;

/**
A scope for [Task]s.

You will only be asked to wait on [Task]s for one pool in a single call.  If there are no restrictions on how tasks can be intermixed for waiting,
you want to use a global pool for the best performance.  Place the [Pool] into a global static so that it is reused
across [crate::Future]s.

In some cases, [Task]s are scoped to a particular resource, such as a hardware device, and [Task]s from different devices
cannot be awaited together in a single call.  If so, you want to create one [Pool] per resource, and use the same pool instance
across all [Task]s with the same resource.  Tasks will be segregated by [Pool] for waiting, so that you are only asked to wait
on one pool's tasks per call.  Maintaining multiple pools has minor performance overheads in some cases but it is necessary
to correctly implement some blocking APIs.
*/
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
            spawn_thread(priority::Priority::UserWaiting, MicroPriority::NEW,  || {worker_fn_user_waiting::<Task>(move_pool_inner) });
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

    To use this method, [Task::Pool] must be Unit.  If not, use [Self::new_with].
*/
    pub fn new() -> Self {
        Self::new_with(())
    }
}
impl<Task: crate::Task> Pool<Task> {
    /**
    Creates a new pool with the specified [Task::Pool] value.  This value will be passed to various methods on the [Task]
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
    /**
    Read the custom type from the pool.
*/
    pub(crate) fn get(&self) -> &Task::Pool {
        &self.send_side_inner.inner.pool_user
    }
}


fn worker_fn_user_waiting<Task: super::Task>(pool_inner: Arc<PoolInner<Task>>) {
    let mut tasks = Vec::new();
    let mut mailboxes = Vec::new();
    //nonblocking task call.  If we block, the thread will shutdown.
    while let Ok(info) = pool_inner.receiver.recv_timeout(Duration::new(0,0)) {
        tasks.push(info.task);
        mailboxes.push(info.mailbox);
        let side_channel = Some(pool_inner.side_channel.read().unwrap());
        'more_tasks: loop {
            let result = Task::wait_any(&pool_inner.pool_user,tasks.as_slice(), side_channel.as_ref().unwrap());
            match result {
                WakeResult::Task(idx, output) => {
                    tasks.remove(idx);
                    let mailbox = mailboxes.remove(idx);
                    unsafe {
                        mailbox.send_mail(output);
                    }
                    if tasks.is_empty() {
                        break 'more_tasks;
                    }
                    else {
                        continue 'more_tasks;
                    }
                }
                WakeResult::Sidechannel => {
                    //refresh the side_channel
                    let new_channel = Task::make_side_channel(&pool_inner.pool_user);
                    std::mem::drop(side_channel); //drop the read lock
                    *pool_inner.side_channel.write().unwrap() = new_channel; //write new channel
                    break 'more_tasks;
                }
            }
        }

    }
    //inform everyone we're shutting down
    pool_inner.thread_launched.store(false, Ordering::Relaxed);
}

pub(crate) struct WorkerSideInfo<Task: crate::Task> {
    pub(crate) task: Task,
   pub(crate) mailbox: Arc<Mailbox<Task::Output>>,
}


