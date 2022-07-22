use std::ffi::c_void;
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicBool, compiler_fence, Ordering};
use std::time::Duration;
use crossbeam_channel::{Receiver, Sender};
use once_cell::sync::OnceCell;
use spawn::{MicroPriority, spawn_thread};
use crate::{Sidechannel, WakeResult};
use crate::future::Mailbox;


/*
This trick lets us return a unique status for every type, avoiding a global lock!
 */
struct ItsFine(*mut c_void /* Points directly to Pool<T>*/);
unsafe impl Send for ItsFine {}
unsafe impl Sync for ItsFine{}

fn once_cell_user_waiting<WorkerSideInfo>() -> &'static OnceCell<ItsFine> {
    static POOL: OnceCell<ItsFine> = OnceCell::new();
    &POOL
}

pub(crate) fn make_pool_if_needed_user_waiting<Task: super::Task>(info: WorkerSideInfo<Task>) -> &'static Pool<Task> {
    let its_fine:  &ItsFine = once_cell_user_waiting::<Task>().get_or_init(|| {
        let (sender, receiver) = crossbeam_channel::bounded(10);
        let pool = Pool {
            sender,receiver,
            thread_launched: AtomicBool::new(false),
            side_channel: RwLock::new(Task::make_side_channel()),
        };
        let mut b = Box::new(pool);
        let r = ItsFine(b.as_mut() as *mut Pool<Task> as *mut c_void);
        //leak box
        std::mem::forget(b);
        r
    });
    let pool = unsafe{&* (its_fine.0 as *const c_void as *const Pool<Task>)};
    pool.sender.send(info).unwrap();
    let thread_launched = pool.thread_launched.swap(true, Ordering::Relaxed);
    if !thread_launched {
        compiler_fence(Ordering::Release); //ensure task was really sent
        spawn_thread(priority::Priority::UserWaiting, MicroPriority::NEW, worker_fn_user_waiting::<Task>);
    }
    else {
        compiler_fence(Ordering::Release); //ensure task was really sent
        pool.side_channel.read().unwrap().wake(); //tell thread about new task
    }
    pool
}

fn get_pool_user_waiting<Task: crate::Task>() -> &'static Pool<Task> {
    unsafe{&*(once_cell_user_waiting::<Task>().get().unwrap().0 as *const c_void as *const Pool<Task>)}
}


fn worker_fn_user_waiting<Task: super::Task>() {
    let pool = get_pool_user_waiting::<Task>();
    let mut tasks = Vec::new();
    let mut mailboxes = Vec::new();
    //nonblocking task call.  If we block, the thread will shutdown.
    while let Ok(info) = pool.receiver.recv_timeout(Duration::new(0,0)) {
        tasks.push(info.task);
        mailboxes.push(info.mailbox);
        let side_channel = Some(pool.side_channel.read().unwrap());
        'more_tasks: loop {
            let result = Task::wait_any(tasks.as_slice(), side_channel.as_ref().unwrap());
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
                    let new_channel = Task::make_side_channel();
                    std::mem::drop(side_channel); //drop the read lock
                    *pool.side_channel.write().unwrap() = new_channel; //write new channel
                    break 'more_tasks;
                }
            }
        }

    }
    //inform everyone we're shutting down
    pool.thread_launched.store(false, Ordering::Relaxed);
}

pub(crate) struct WorkerSideInfo<Task: crate::Task> {
    pub(crate) task: Task,
   pub(crate) mailbox: Arc<Mailbox<Task::Output>>,
}


pub struct Pool<Task: crate::Task> {
    receiver: Receiver<WorkerSideInfo<Task>>,
    sender: Sender<WorkerSideInfo<Task>>,
    thread_launched: AtomicBool,
    //todo: I suspect there is some way to do this with atomics but I'm not certain
    side_channel: RwLock<Task::Sidechannel>,
}
