use std::os::unix::io::{RawFd, IntoRawFd};
use dispatchr::external_data::{HasMemory, ExternalMemory};
use dispatchr::queue::Unmanaged;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::sync::{Mutex, Arc};
use dispatchr::io::dispatch_fd_t;
use dispatchr::data::DispatchData;
use crate::io::stream::OSError;

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

pub struct OSWriteOptions<'a> {
    queue: &'a Unmanaged
}
impl<'a> OSWriteOptions<'a> {
    pub fn new(queue: &'a Unmanaged) -> OSWriteOptions {
        OSWriteOptions {
            queue
        }
    }
}

struct WriteTask {
    waker: Option<Waker>,
    result: Option<i32>,
}


struct WriteFuture<'a, DataType:DispatchData> {
    data: DataType,
    options: OSWriteOptions<'a>,
    fd: RawFd,
    started: bool,
    working: Arc<Mutex<WriteTask>>
}
impl<'a, DataType: DispatchData + std::marker::Unpin> Future for WriteFuture<'a, DataType> {
    type Output = Result<(),OSError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut_self = self.get_mut();
        if !mut_self.started {
            mut_self.working.lock().unwrap().waker = Some(cx.waker().clone());
            mut_self.started = true;
            let move_lock = mut_self.working.clone();
            dispatchr::io::write(dispatch_fd_t::new(mut_self.fd), &mut_self.data, &*mut_self.options.queue, move |_a,b| {
                let mut lock = move_lock.lock().unwrap();

                lock.result = Some(b);
                lock.waker.take().unwrap().wake();

            });
            Poll::Pending
        }
        else {
            let mut final_lock = mut_self.working.lock().unwrap();
            if let Some(result) = final_lock.result {
                if result == 0 {
                    Poll::Ready(Ok(()))
                }
                else {
                    Poll::Ready(Err(OSError(result)))
                }
            }
            else {
                final_lock.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}


impl Write {
    pub fn new<T: IntoRawFd>(fd: T) -> Self{
        Write {
            fd: fd.into_raw_fd()
        }
    }
    ///A fast path to write static data.
    pub fn write_static<'a>(&self, buffer: &'static [u8], write_options: OSWriteOptions<'a>) -> impl Future<Output=Result<(),OSError>> + 'a {
        let buffer = ExternalMemory::new(StaticBuffer(buffer), write_options.queue);
        let future = WriteFuture {
            data: buffer,
            options: write_options,
            fd: self.fd,
            started: false,
            working: Arc::new(Mutex::new(WriteTask { waker: None, result: None }))
        };
        future
    }
}

#[test] fn write_t() {
    let path = std::path::Path::new("/tmp/kiruna_write_t.txt");
    let file = std::fs::File::create(path).unwrap();

    let write = Write::new(file);
    let future = write.write_static("hello from the test".as_bytes(), OSWriteOptions::new(dispatchr::queue::global(dispatchr::QoS::UserInitiated).unwrap()));
    let result = crate::test::test_await(future, std::time::Duration::from_secs(1));
    assert!(result.is_ok());

    let read_file = std::fs::read(path);
    assert_eq!(read_file.unwrap().as_slice(), "hello from the test".as_bytes());
}