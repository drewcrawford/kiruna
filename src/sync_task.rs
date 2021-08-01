use std::future::Future;
use std::task::{Waker, Wake};
use std::sync::Arc;
use std::sync::mpsc::SyncSender;

///Top-level future
struct SyncTask<FutureType: Future> {
    future: SyncFuture<FutureType>,
    channel: SyncSender<Arc<SyncTask<FutureType>>>
}

///Wrapper type that declares our future is threadsafe.
///This should be safe from single-thread executors (but not in general!)
/// So it should not be exported out of here
struct SyncFuture<FutureType: Future>(FutureType);
unsafe impl<F: Future> Send for SyncFuture<F> {}
unsafe impl<F: Future> Sync for SyncFuture<F> {}

impl<F: Future> Wake for SyncTask<F> {
    fn wake(self: Arc<Self>) {
        //unclear to me if this is the most performant strategy
        let channel = self.channel.clone();
        channel.send(self).unwrap();
    }
}

impl<F: Future + 'static> SyncTask<F> {
    fn into_waker(self: Arc<SyncTask<F>>) -> Waker {
        self.into()
    }
}