use std::os::unix::io::{IntoRawFd, RawFd};
use crate::{OSError, PriorityDispatch};
use dispatchr::io::dispatch_fd_t;
use std::future::Future;
use dispatchr::io::read_completion;
use priority::Priority;
use dispatchr::data::{Managed, Unmanaged, DispatchData};
use r#continue::continuation;

///Buffer type.
///
/// This is an opaque buffer managed by kiruna.
#[derive(Debug)]
pub struct ReadBuffer(pub(crate) Managed);
impl ReadBuffer {
    pub fn as_dispatch_data(&self) -> &dispatchr::data::Unmanaged {
        self.0.as_unmanaged()
    }
    pub fn into_contiguous(self) -> ContiguousBuffer {
        //move out of self here
        ContiguousBuffer(dispatchr::data::Contiguous::new(self.0 ))
    }
    pub(crate) fn add(&mut self, tail: &Unmanaged) {
        self.0 = self.0.as_unmanaged().concat(tail)
    }
}

pub struct ContiguousBuffer(dispatchr::data::Contiguous);
impl ContiguousBuffer {
    pub fn as_dispatch_data(&self) -> &dispatchr::data::Unmanaged {
        self.0.as_dispatch_data()
    }
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}

///Backend-specific read options.  On macOS, you may target a specific queue
/// for the reply to read calls.
///
/// For cases where you intend to target a global queue, it may be more convenient to use [Priority] instead, which
/// is convertible (often, implicitly) to this type.
#[derive(Clone)]
pub struct OSOptions<'a> {
    ///Queue (QoS) for performing I/O
    queue: &'a dispatchr::queue::Unmanaged
}
impl<'a> OSOptions<'a> {
    pub fn new(queue: &'a dispatchr::queue::Unmanaged) -> Self {
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
    ///Performs a single read.
    ///
    /// In practice, this function reads 0 bytes if the stream is closed.
    fn once<'a, O: Into<OSOptions<'a>>>(&self, os_read_options: O) -> impl Future<Output=Result<Managed,OSError>> {
        let (sender,receiver) = continuation();
        read_completion(dispatch_fd_t::new(self.fd), usize::MAX, os_read_options.into().queue, |data,err| {
            if err==0 {
                sender.send(Ok(Managed::retain(data)))
            }
            else {
                sender.send(Err(OSError(err)))
            }
        });
        receiver
    }

    ///Reads the entire fd into memory
    pub async fn all<'a, O: Into<OSOptions<'a>>>(&self, os_read_options: O) -> Result<ReadBuffer,OSError> {
        let mut buffer = ReadBuffer(Managed::retain(Unmanaged::new()));
        let read_options = os_read_options.into();
        loop {
            let new_data = self.once(read_options.clone()).await?;
            if new_data.as_unmanaged().len() == 0 {
                break
            }
            else {
                buffer.add(new_data.as_unmanaged());
            }
        }
        Ok(buffer)
    }


}
#[test] fn test() {
    use kiruna::test::test_await;
    use std::time::Duration;
    // println!("{:?}",std::env::current_dir());
    let path = std::path::Path::new("src/macos.rs");
    let file = std::fs::File::open(path).unwrap();
    let read = Read::new(file);
    let buffer = test_await(read.all(Priority::Testing), Duration::from_secs(2));
    assert!(buffer.unwrap().into_contiguous().as_slice().starts_with("// FIND-ME".as_bytes()));
}
#[test] fn multiple_passes() {
    use core::ffi::c_void;
    //Write in multiple pieces and ensure we get all of it
    //mt2-130
    let mut pipes = [0,0];
    let pipe = unsafe{ libc::pipe(&mut pipes as *mut _) };
    assert!(pipe >= 0);
    let read_struct = Read::new(pipes[0]);
    let read_all = read_struct.all(super::Priority::Testing);
    std::thread::spawn(move || {
        for item in 0..10 {
            let str_strong = format!("{}\n",item);
            let str = str_strong.as_bytes();
            unsafe{ libc::write(pipes[1], str as *const _ as *const c_void, str.len());}
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        unsafe{ libc::close(pipes[1]) };
    });
    let read = kiruna::test::test_await(read_all, std::time::Duration::from_secs(10));

    let mut expected = String::new();
    for item in 0..10 {
        expected.push_str(&format!("{}\n",item));
    }
    let expected_bytes = expected.as_bytes();
    assert_eq!(read.unwrap().into_contiguous().as_slice(), expected_bytes)
}