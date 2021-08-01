use std::future::Future;
use std::sync::mpsc::SyncSender;
use std::cell::UnsafeCell;
use std::rc::Rc;

///Top-level future
pub(crate) struct SyncTask {
    future: Rc<dyn Future<Output=()>>,
    channel: SyncSender<Rc<dyn Future<Output=()>>>,
}



impl SyncTask {
    pub(crate) fn new<F: Future<Output=()> + 'static>(future: F, channel: SyncSender<Rc<dyn Future<Output=()>>>) -> Self {

        SyncTask {
            future: Rc::new(future
            ),
            channel

        }
    }

    fn wake(self: Rc<Self>) {
        self.channel.send(self.future.clone()).unwrap();
    }
}

///Wrapper type that declares our future is threadsafe.
///This should be safe from single-thread executors (but not in general!)
/// So it should not be exported out of here
struct UnsafeSync<T>(T);
unsafe impl<F> Send for UnsafeSync<F> {}
unsafe impl<F> Sync for UnsafeSync<F> {}
impl<T> UnsafeSync<T> {
    unsafe fn new(t: T) -> Self {
        Self(t)
    }
}

