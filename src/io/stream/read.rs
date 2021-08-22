use std::os::unix::io::{IntoRawFd, RawFd};
use crate::io::stream::{Buffer, OSError};
use dispatchr::io::dispatch_fd_t;
use std::future::Future;
use dispatchr::io::read_completion;
use crate::Priority;
use dispatchr::data::{Contiguous};

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


impl Read {
    pub fn new<T: IntoRawFd>(fd: T) -> Read {
        Read {
            fd: fd.into_raw_fd()
        }
    }

    ///Reads the entire fd into memory
    pub fn all<'a, O: Into<OSReadOptions<'a>>>(&self, os_read_options: O) -> impl Future<Output=Result<Buffer,OSError>> {
        let (continuation, completion) = blocksr::continuation::Continuation::<(),_>::new();
        read_completion(dispatch_fd_t::new(self.fd), usize::MAX, os_read_options.into().queue, |data,err| {
            if err==0 {
                completion.complete(Ok(Buffer(Contiguous::new(data))))
            }
            else {
                completion.complete(Err(OSError(err)))
            }
        });
        continuation
    }


}
#[test] fn test() {
    use crate::test::test_await;
    use std::time::Duration;
    let path = std::path::Path::new("src/io/stream.rs");
    let file = std::fs::File::open(path).unwrap();
    let read = Read::new(file);
    let buffer = test_await(read.all(Priority::Testing), Duration::from_secs(2));
    assert!(buffer.unwrap().as_slice().starts_with("// FIND-ME".as_bytes()));
}