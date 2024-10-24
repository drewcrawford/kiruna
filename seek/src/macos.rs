use std::fs::File;
use std::future::Future;
use std::os::raw::c_int;
use std::os::unix::io::{AsFd, AsRawFd};
use std::path::{Path};
use dispatchr::data::{Contiguous, DispatchData, Unmanaged};
use dispatchr::io::{dispatch_io_type_t, IO};
use dispatchr::QoS;
use pcore::release_pool::ReleasePool;
use r#continue::continuation;
use priority::Priority;
use crate::imp::Error::DispatchError;

pub struct Buffer(Contiguous);
impl Buffer {
    pub fn as_dispatch_data(&self) -> &Unmanaged {
        self.0.as_dispatch_data()
    }
}
#[derive(Debug)]
pub struct Read {
    _file: File,
    io: IO,
    priority: Priority,
}
#[derive(Debug,thiserror::Error)]
pub enum Error{
    #[error("libdispatch error {0}")]
    DispatchError(c_int),
    #[error("io error {0}")]
    IoError(#[from] std::io::Error),
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
        let (sender,receiver) = continuation();

        dispatchr::io::read_completion(dispatchr::io::dispatch_fd_t::new(file), usize::MAX, queue, move |data, err| {
            if err != 0 {
                sender.send(Err(err));
            }
            else {
                let managed = dispatchr::data::Managed::retain(data);
                sender.send(Ok(managed))
            }
        });
        async {
            let r = receiver.await;
            r.map(|d| Buffer(Contiguous::new(d))).map_err(|e| DispatchError(e))
        }
    }
    pub fn new(path: &Path, priority: Priority) -> impl Future<Output=Result<Self,Error>> {
        let queue = match priority {
            priority::Priority::UserWaiting | priority::Priority::Testing => {
                dispatchr::queue::global(QoS::UserInitiated).unwrap()
            }
            _ => todo!(),
        };
        let file = File::open(path).unwrap();
        let fd = file.as_fd().as_raw_fd();
        let io = dispatchr::io::IO::new(dispatch_io_type_t::RANDOM, dispatchr::io::dispatch_fd_t::new(fd), queue).unwrap();
        async move {
            Ok(Self {
                _file: file,
                io,
                priority,
            })
        }

    }
    ///Reads a slice of bytes from the file at the specified offset.
    ///
    /// The offset is relative to the start of the file.
    pub fn read(&mut self, offset: usize, size: usize) -> impl Future<Output=Result<Buffer,Error>> + Send {
        let queue = match self.priority {
            priority::Priority::UserWaiting | priority::Priority::Testing => {
                dispatchr::queue::global(QoS::UserInitiated).unwrap()
            }
            _ => todo!(),
        };
        let (sender,receiver) = continuation();
        struct Environment {
            //option is for take
            data: Option<dispatchr::data::Managed>,
            //option is for take
            completer: Option<r#continue::Sender<Result<dispatchr::data::Managed,i32>>>,
        }
        self.io.read(offset.try_into().unwrap(), size, queue, |environment, done, data, err| {
            if err != 0 {
                environment.completer.take().unwrap().send(Err(err));
            }
            else {
                let concat = environment.data.take().unwrap().as_unmanaged().concat(unsafe{&*data});
                environment.data = Some(concat);
                if done {
                    environment.completer.take().unwrap().send(Ok(environment.data.take().unwrap()))
                }
            }
        }, Environment {
            data: Some(dispatchr::data::Managed::retain(dispatchr::data::Unmanaged::new())),
            completer: Some(sender),
        });
        async {
            let r = receiver.await;
            r.map(|d| Buffer(Contiguous::new(d))).map_err(|e| DispatchError(e))
        }
    }
    pub async fn async_clone(&self, priority: priority::Priority) -> Result<Self,Error> {
        Ok(
            Self {
                //since this is only used to keep the file open, I think there is no problem with re-using it.
                _file: self._file.try_clone()?,
                io: self.io.clone(),
                priority
            }
        )
    }
}