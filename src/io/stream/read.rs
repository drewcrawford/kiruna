use std::os::unix::io::{IntoRawFd, RawFd};
use std::task::{Poll, Waker, Context};
use crate::io::stream::Buffer;
use dispatchr::io::dispatch_fd_t;
use std::sync::{Mutex, Arc};
use std::future::Future;
use std::pin::Pin;
use dispatchr::io::read;
use crate::Priority;

///Backend-specific read options.  On macOS, you may target a specific queue
/// for the reply to read calls.
///
/// For cases where you intend to target a global queue, it may be more convenient to use [Priority] instead, which
/// is convertible (often, implicitly) to this type.
#[derive(Clone)]
pub struct OSReadOptions<'a> {
    ///Queue (QoS) for performing I/O
    queue: &'a dispatchr::queue::Unmanaged
}
impl<'a> OSReadOptions<'a> {
    pub fn new(queue: &'a dispatchr::queue::Unmanaged) -> Self {
        OSReadOptions {
            queue
        }
    }
}

impl From<Priority> for OSReadOptions<'static> {
    fn from(priority: Priority) -> Self {
        let queue = dispatchr::queue::global(priority.as_qos()).unwrap();
        OSReadOptions {
            queue: queue
        }
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
pub struct DispatchReadFuture<'a> {
    fd: dispatch_fd_t,
    size: usize,
    queue: &'a dispatchr::queue::Unmanaged,
    result: Arc<Mutex<DispatchFutureResult>>,
    started: bool
}
impl<'a> Future for DispatchReadFuture<'a> {
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
                read(self.fd, self.size, self.queue, move |a,b| {
                    let mut lock = capture_arc.lock().unwrap();
                    if b == 0 {
                        lock.result = Some(Buffer(a.as_contiguous()));
                    }
                    lock.waker.take().unwrap().wake();
                });
            }
            Poll::Pending
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
    pub fn all<'a, O: Into<OSReadOptions<'a>>>(&self, os_read_options: O) -> DispatchReadFuture<'a> {
        DispatchReadFuture {
            fd: dispatch_fd_t::new(self.fd),
            size: usize::MAX,
            queue: os_read_options.into().queue,
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
    let buffer = test_await(read.all(Priority::Testing), Duration::from_secs(2));
    assert!(buffer.as_slice().starts_with("// FIND-ME".as_bytes()));
}