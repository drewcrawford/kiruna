// FIND-ME
/*! Provides streaming IO.  See [io] for a comparison of io types.
*/
#[cfg(not(feature ="stream_with_dispatch"))] compile_error!("Need to specify a backend");

use dispatchr::data::Contiguous;
use std::os::unix::io::{RawFd, IntoRawFd};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use dispatchr::io::{read, dispatch_fd_t};
use dispatchr::queue::{Unmanaged};
use std::sync::{Mutex, Arc};

///Buffer type.
///
/// This is an opaque buffer managed by kiruna.
#[derive(Debug)]
pub struct Buffer(Contiguous);
impl Buffer {
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}

///Reads from a file descriptor
pub struct Read {
    fd: RawFd
}

///Internal result type for dispatch
struct DispatchFutureResult {
    waker: Option<Waker>,
    result: Option<Buffer>,
}

///Future for dispatch_read
pub struct DispatchBufferFuture {
    fd: dispatch_fd_t,
    size: usize,
    queue: Unmanaged,
    result: Arc<Mutex<DispatchFutureResult>>,
    started: bool
}
impl Future for DispatchBufferFuture {
    type Output = Buffer;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let result = self.result.lock().unwrap().result.take();
        if let Some(buffer) = result {
            Poll::Ready(buffer)

        }
        else {
            //set new waker
            self.result.lock().as_mut().unwrap().waker = Some(cx.waker().clone());
            if !self.started {
                //this needs to be set up front to guarantee that we don't get another call
                {
                    self.started = true;
                }
                let capture_arc = self.result.clone();
                read(self.fd, self.size, self.queue.clone(), move |a,b| {
                    let mut lock = capture_arc.lock().unwrap();
                    if b == 0 {
                        lock.result = Some(Buffer(a.into_contiguous()));
                    }
                    lock.waker.take().unwrap().wake();
                });
            }
            Poll::Pending
        }

    }
}

#[derive(Clone)]
pub struct OSReadOptions {
    ///Queue (QoS) for performing I/O
    queue: Unmanaged
}
impl OSReadOptions {
    pub fn new(queue: Unmanaged) -> OSReadOptions {
        OSReadOptions {
            queue: queue
        }
    }
}


impl Read {
    pub fn new<T: IntoRawFd>(fd: T) -> Read {
        Read {
            fd: fd.into_raw_fd()
        }
    }

    ///Reads the entire fd into memory
    pub fn all(&self, os_read_options: OSReadOptions) -> DispatchBufferFuture {
        DispatchBufferFuture {
            fd: dispatch_fd_t::new(self.fd),
            size: usize::MAX,
            queue: os_read_options.queue,
            result: Arc::new(Mutex::new(DispatchFutureResult { waker: None, result: None })),
            started: false
        }
    }


}

#[test] fn test() {
    use crate::test::test_await;
    use std::time::Duration;
    let path = std::path::Path::new("src/io/stream.rs");
    let file = std::fs::File::open(path).unwrap();
    let read = Read::new(file);
    let buffer = test_await(read.all(OSReadOptions{queue: global(QoS::UserInitiated)}), Duration::from_secs(2));
    assert!(buffer.as_slice().starts_with("// FIND-ME".as_bytes()));
}