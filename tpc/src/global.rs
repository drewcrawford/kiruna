use std::sync::atomic::{AtomicU64, Ordering};
use once_cell::sync::OnceCell;
use crate::platform::threadpool_size;

///Special statistics type, can be encoded into a u64 for atomic ops.
///
/// These threads have been created and we expect them to run soon.
#[derive(Debug)]
pub struct ThreadCounts {
    pub user_waiting: u16,
}
impl ThreadCounts {
    fn new() -> ThreadCounts { Self { user_waiting: 0}}
}
impl From<u64> for ThreadCounts {
    fn from(u: u64) -> Self {
        ThreadCounts {
            user_waiting: (u & 0xffffffff) as u16
        }
    }
}
impl From<ThreadCounts> for u64 {
    fn from(t: ThreadCounts) -> Self {
        t.user_waiting as u64
    }
}

pub struct GlobalState {
    pub physical_cpus: u16,
    //encoding of ThreadCounts type.
    pub thread_counts: AtomicU64,
}
impl GlobalState {
    pub fn read_thread_counts(&self) -> ThreadCounts {
        self.thread_counts.load(Ordering::Relaxed).into()
    }
    pub fn update_thread_counts<F: FnMut(&mut ThreadCounts)>(&self, mut atomic_fn: F) -> Result<ThreadCounts,ThreadCounts> {
        self.thread_counts.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |arg| {
            let mut thread_counts: ThreadCounts = arg.into();
            atomic_fn(&mut thread_counts);
            let out_arg = thread_counts.into();
            if arg != out_arg {
                Some(out_arg)
            }
            else {
                None
            }
        }).map(|e| e.into()).map_err(|e| e.into())
    }
    pub fn global() -> &'static GlobalState {
        static GLOBAL_STATE: OnceCell::<GlobalState> = OnceCell::new();
        GLOBAL_STATE.get_or_init( ||{
            GlobalState::new()
        })
    }
    pub fn new() -> GlobalState {
        let physical_cpus = threadpool_size();
        GlobalState {
            physical_cpus: physical_cpus,
            thread_counts: AtomicU64::new(ThreadCounts::new().into()),
        }
    }
}