use std::os::unix::io::{RawFd, IntoRawFd};
use dispatchr::external_data::{HasMemory, ExternalMemory};
use dispatchr::queue::Unmanaged;
use std::future::Future;
use dispatchr::io::dispatch_fd_t;
use dispatchr::data::{DispatchData};
use crate::{OSError, PriorityDispatch};
use priority::Priority;

pub struct Write {
    fd: RawFd
}

///Fast path for static data
struct StaticBuffer(&'static [u8]);


impl HasMemory for StaticBuffer {
    fn as_slice(&self) -> &[u8] {
        self.0
    }
}

struct BoxedBuffer(Box<[u8]>);
impl HasMemory for BoxedBuffer {
    fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

///Backend-specific write options.  On macOS, you may target a specific queue
/// for the reply to write calls.
///
/// For cases where you intend to target a global queue, it may be more convenient to use [Priority] instead, which
/// is convertible (often, implicitly) to this type.
pub struct OSOptions<'a> {
    queue: &'a Unmanaged
}
impl<'a> OSOptions<'a> {
    pub fn new(queue: &'a Unmanaged) -> OSOptions {
        OSOptions {
            queue
        }
    }
}
impl From<Priority> for OSOptions<'static> {
    fn from(priority: Priority) -> Self {
        let queue = dispatchr::queue::global(priority.as_qos()).unwrap();
        OSOptions {
            queue: queue
        }
    }
}


impl Write {
    pub fn new<T: IntoRawFd>(fd: T) -> Self{
        Write {
            fd: fd.into_raw_fd()
        }
    }
    fn write_data<'a>(&self, buffer: ExternalMemory, write_options: OSOptions<'a>) -> impl Future<Output=Result<(), OSError>> + 'a{
        let (continuation,completer) = blocksr::continuation::Continuation::<(),_>::new();
        dispatchr::io::write_completion(dispatch_fd_t::new(self.fd), &buffer.as_unmanaged(), write_options.queue, |_data, err| {
            if err == 0 {
                completer.complete(Ok(()))
            }
            else {
                completer.complete(Err(OSError(err)))
            }
        });
        continuation
    }
    ///A fast path to write static data.
    pub fn write_static<'a,O: Into<OSOptions<'a>>>(&self, buffer: &'static [u8], write_options: O) -> impl Future<Output=Result<(),OSError>> + 'a{
        let as_write_options = write_options.into();
        let buffer = ExternalMemory::new(StaticBuffer(buffer), as_write_options.queue);
        self.write_data(buffer, as_write_options)
    }
    ///A path that writes heap-allocated data.
    pub async fn write_boxed<'a, O: Into<OSOptions<'a>>>(&self, buffer: Box<[u8]>, write_options: O) -> Result<(),OSError> {
        let as_write_options = write_options.into();
        let buffer = ExternalMemory::new(BoxedBuffer(buffer), as_write_options.queue);
        self.write_data(buffer, as_write_options).await
    }
}

#[test] fn write_t() {
    let path = std::path::Path::new("/tmp/kiruna_write_t.txt");
    let file = std::fs::File::create(path).unwrap();

    let write = Write::new(file);
    let future = write.write_static("hello from the test".as_bytes(), Priority::Testing);
    let result = kiruna::test::test_await(future, std::time::Duration::from_secs(1));
    assert!(result.is_ok());

    let read_file = std::fs::read(path);
    assert_eq!(read_file.unwrap().as_slice(), "hello from the test".as_bytes());
}