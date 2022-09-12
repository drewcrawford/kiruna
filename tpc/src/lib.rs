/*!
An asymmetric, simple, per-core, cooperative async executor.

# STATUS

This is very early, do not expect it to work

# Overview

This is (will be) kiruna's recommended parallel executor.  What the `sync` executor is to single-core workloads,
this executor is to multicore workloads.

tpc is an appropriate choice for both sync and async tasks.  It is appropriate for high-performance and energy-efficient workoads.
It scales to the maximum performance available on the system.

tcp is (will be) appropriate for windows and macOS applications, and can be easily extended to other platforms.

# Asymmetric

tpc is designed to handle asymmetric workloads and asymmetric hardware.  For example, it can handle high-priority
work such as rendering tasks, at the same time as low-priority IO or caching.  It can divide work between asymmetric cores, such
as performance and efficiency cores.

Of course, asymmetric designs are strictly the harder case.  tpc also works great for symmetric workloads and hardware.

# Simple

tpc is a thin-ish layer atop the OS scheduler.  tpc performs basic application-level scheduling and then uses the OS to do
hardware-layer scheduling.  This leverages all the work the kernel and hardware folks do including for
example the Intel Thread Director, mach scheduling, and many more.  kiruna priorities are directly translated into thread priorities,
so the OS has good scheduling intelligence.

tpc can be debugged with native platform tools in the obvious way.

tpc can be easily ported to new platforms that support basic OS-level concepts such as threads and priority.

# Per core

tpc implements a "per core" model.  We try to limit concurrency to the actual cores on the system.  This is similar to, but
slightly different than the better-known "thread per core", see the second on the threading model below.

This means that you can, and should, enqueue into tpc an unlimited number of tasks without fear of spinning up too many threads.

# Cooperative

One limitation of the per core model is that we limit the number of new threads.  Therefore, if a task were to block,
it would constrain performance of the overall system.

For this reason, you really ought to use nonblocking IO, and void blocking calls such as locks, for any tasks sent to tpc.
If you somehow fill the whole queue with tasks that block, tpc may be unable to make forward progress, deadlocking your program.

That said, a bit of IO is not going to kill you on most modern systems.  Just try not to make it a habit.

# Task model

Each task sent to the executor is assigned a [Priority].  This is used in two ways:

* tpc itself strictly prefers higher-priority tasks to lower-priority tasks.  For example, given a choice between two tasks, tpc begins
  the task of higher priority.
* tpc's worker threads have an OS priority which is derived from the task's [Priority].  This allows the OS to schedule
  at the thread level as desired.  Typically, the OS will send low-priority threads to more efficient cores even if more performant
  cores are available.  The OS may also interleave threads with different priorities as it determines is appropriate.

# Thread model

From a user perspective, the particular thread in which a task runs is not defined.  A task can also be moved
from one thread to another across any suspension point.

tpc attempts to use a small number of threads.  Threads are pretty expensive: they have substantial memory overhead,
take tens of microseconds to start, then the OS has to worry about scheduling them, etc.

Therefore, tpc sharply limits the number of threads it creates while still having sufficient concurrency.
It generally tries to maintain a thread per core, although in certain access patterns you may wind up with more temporarily.

The general algorithm is:

1.  You send a task at some priority.
2.  Your priority is binned into some small number of bins.  This is to promote work-stealing from related priorities.
2.  If the task is not blocked by higher-priority work, a thread in the bin will accept the task and adopt the appropriate OS priority for the task.
3.  If there are fewer tasks than cores, tpc will ensure there is a hot spare to accept new tasks.  You can influence the number of hot spares by dispatching
    all tasks in a single call.
4.  When there is too much work at a given priority level, tasks will enqueue and wait for threads to be available.
5.  When threads run out of work, they will become hot spares to accept new tasks.
6.  After some time, idle threads will be terminated.


*/

extern crate core;

mod bin;

mod platform;
mod global;
pub mod set;

#[macro_use]
mod stories;

use std::future::Future;
use std::pin::Pin;
use priority::Priority;
use crate::bin::{Bin, SimpleJob};


pub struct Executor;

//type HeapFuture = Pin<Box<dyn Future<Output=()> + Send>>;


impl Executor {

    /**
    Spawns the tasks at the specified priority.
    .*/
    pub fn spawn_mixed<const LENGTH: usize>(&'static self, priority: Priority, futures: [Pin<Box<(dyn Future<Output=()> + Send)>>; LENGTH]) {
        match priority {
            Priority::UserWaiting | Priority::Testing => {
                Bin::user_waiting().spawn_mixed(futures)
            }
            _ => {
                panic!("Unsupported priority {:?}",priority);
            }
        }
    }

    pub fn spawn_mixed_simple<const LENGTH: usize>(&'static self, priority: Priority, jobs: [SimpleJob; LENGTH]) {
        match priority {
            Priority::UserWaiting | Priority::Testing => {
                Bin::user_waiting().spawn_mixed_simple(jobs)
            }
             _ => {
                 panic!("Unsupported prority {:?}",priority);
             }
        }
    }


    pub fn global() -> &'static Executor { &Executor }

    fn bin_for(&'static self, priority: Priority) -> &'static Bin {
        match priority {
            Priority::UserWaiting | Priority::Testing => {
                Bin::user_waiting()
            }
            _ => {
                panic!("Unsuported priority {:?}",priority);
            }
        }
    }
}

#[test] fn test_spawn() {
    let (sender,receiver) = std::sync::mpsc::sync_channel(10);

   Executor::global().spawn_mixed(priority::Priority::Testing, [Box::pin(async move {
        sender.send(()).unwrap();
    })]);
    receiver.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
}