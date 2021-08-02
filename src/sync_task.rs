use std::future::{Future};
use std::cell::UnsafeCell;
use std::task::{Wake, Context, Poll, Waker};
use std::sync::{Arc};
use std::pin::Pin;
use std::sync::mpsc::Sender;

///Top-level future
pub(crate) struct SyncTask {
    future: UnsafeCell<Pin<Box<dyn Future<Output=()>>>>,
    channel: Sender<ChannelType>,
}

impl SyncTask {
    pub(crate) fn new<F: Future<Output=()> + 'static>(future: F, channel: Sender<ChannelType>) -> Self {
        SyncTask {
            future: UnsafeCell::new(Box::pin(future
            )),
            channel: channel

        }
    }
    pub(crate) fn begin(self) {
        println!("begin task");
        Arc::new(SyncWake{inner: self}).wake()
    }
}

pub(crate) struct SyncWake {
    inner: SyncTask,
}
//We are implementing these even though the type is not really threadsafe.
//The implication is, any fn that reads or writes to these fields, must be unsafe.
//Caller must verify that these are never called on multiple threads.
unsafe impl Send for SyncWake { }
unsafe impl Sync for SyncWake { }
pub(crate) type ChannelType = Arc<SyncWake>;
impl Wake for SyncWake {
    fn wake(self: Arc<Self>) {
        //I "believe" this is safe because channel is never mutated.
        //so this funciton can be called by any thread, meaning it doesn't have to be unsafe.
        self.inner.channel.send(self.clone()).unwrap();
    }
}
impl SyncWake {
    ///- safety: You must ensure this function is always called from the same thread.
    pub(crate) unsafe fn poll(self: Arc<Self>) -> Poll<()> {
        let as_waker: Waker = self.clone().into();
        let mut context = Context::from_waker(&as_waker);
        let future_slot = &mut *self.inner.future.get();

        let result = future_slot.as_mut().poll(&mut context);
        result
    }
}