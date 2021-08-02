use std::future::{Future};
use std::cell::UnsafeCell;
use std::task::{Context, Poll};
use std::sync::{Arc};
use std::pin::Pin;
use std::sync::mpsc::{Sender};
use super::executor::Handle;

///Top-level future
pub(crate) struct Task {
    future: UnsafeCell<Pin<Box<dyn Future<Output=()>>>>,
}

impl Task {
    pub(crate) fn new<F: Future<Output=()> + 'static>(future: F) -> Self {
        Task {
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
pub(crate) struct ChannelFactory {
    inner: Sender<Handle>
}

unsafe impl Send for ChannelFactory {}
unsafe impl Sync for ChannelFactory {}
impl ChannelFactory {
    ///Safety: pass an unused channel in here
    pub(crate) unsafe fn new(channel: Sender<Handle>) -> ChannelFactory {
        ChannelFactory {
            inner: channel
        }
    }
}
impl ChannelFactory {
    fn new_channel(&self) -> Sender<Handle> {
        self.inner.clone()
    }
}
pub(crate) struct Wake {
    handle: Handle,
    channel: ChannelFactory
}
impl Wake {
    pub(crate) fn new(handle: Handle, channel_factory: ChannelFactory) -> Wake {
        Wake {
            handle: handle,
            channel: channel_factory
        }
    }
}
impl std::task::Wake for Wake {
    fn wake(self: Arc<Self>) {
        self.channel.new_channel().send(self.handle).unwrap();
    }
}
