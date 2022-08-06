use std::fs::File;
use std::future::Future;
use std::os::raw::c_int;
use std::path::Path;
use blocksr::continuation::Continuation;
use dispatchr::data::Contiguous;
use dispatchr::QoS;
use pcore::release_pool::ReleasePool;
use priority::Priority;
use crate::imp::Error::DispatchError;

pub struct Buffer(Contiguous);
pub struct Read;
#[derive(Debug,thiserror::Error)]
pub enum Error{
    #[error("libdispatch error {0}")]
    DispatchError(c_int),
}

impl Buffer {
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl Read {
    /**
    Asynchronous read; reads the entire contents of a file.
     */
    pub fn all(path: &Path, priority: Priority, _release_pool: &ReleasePool) -> impl Future<Output=Result<Buffer,Error>> {
        let queue = match priority {
            priority::Priority::UserWaiting | priority::Priority::Testing => {
                dispatchr::queue::global(QoS::UserInitiated).unwrap()
            }
            _ => todo!(),
        };
        let file = File::open(path).unwrap();
        let (continuation,completer) = Continuation::<(), _>::new();
        dispatchr::io::read_completion(dispatchr::io::dispatch_fd_t::new(file), usize::MAX, queue, move |data, err| {
            if err != 0 {
                completer.complete(Err(err));
            }
            else {
                let managed = dispatchr::data::Managed::retain(data);
                completer.complete(Ok(managed))
            }
        });
        async {
            let r = continuation.await;
            r.map(|d| Buffer(Contiguous::new(d))).map_err(|e| DispatchError(e))
        }
    }
}