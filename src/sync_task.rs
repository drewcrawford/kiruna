use std::future::{Future};
use std::cell::UnsafeCell;
use std::task::{Wake, Context, Poll};
use std::sync::{Arc};
use std::pin::Pin;
use std::sync::mpsc::{Sender};
use crate::sync_executor::SyncExecutorHandle;

///Top-level future
pub(crate) struct SyncTask {
    future: UnsafeCell<Pin<Box<dyn Future<Output=()>>>>,
}

impl SyncTask {
    pub(crate) fn new<F: Future<Output=()> + 'static>(future: F) -> Self {
        SyncTask {
            future: UnsafeCell::new(Box::pin(future
            )),
        }
    }
    ///Must call this from the same thread each time
    pub(crate) unsafe fn poll(&self, cx: &mut Context) -> Poll<()> {
        let future = &mut *self.future.get();
        future.as_mut().poll(cx)
    }
}

///Ordinarily, channels cannot be shared across threads.  However, we can clone
/// a channel and use it from one thread.
///
/// To use this correctly, you must pass an unused channel into this type.
#[derive(Clone)]
pub struct ChannelFactory {
    inner: Sender<SyncExecutorHandle>
}

unsafe impl Send for ChannelFactory {}
unsafe impl Sync for ChannelFactory {}
impl ChannelFactory {
    ///Safety: pass an unused channel in here
    pub(crate) unsafe fn new(channel: Sender<SyncExecutorHandle>) -> ChannelFactory {
        ChannelFactory {
            inner: channel
        }
    }
}
impl ChannelFactory {
    fn new_channel(&self) -> Sender<SyncExecutorHandle> {
        self.inner.clone()
    }
}
pub(crate) struct SyncWake {
    handle: SyncExecutorHandle,
    channel: ChannelFactory
}
impl SyncWake {
    pub(crate) fn new(handle: SyncExecutorHandle, channel_factory: ChannelFactory) -> SyncWake {
        SyncWake {
            handle: handle,
            channel: channel_factory
        }
    }
}
impl Wake for SyncWake {
    fn wake(self: Arc<Self>) {
        self.channel.new_channel().send(self.handle).unwrap();
    }
}
