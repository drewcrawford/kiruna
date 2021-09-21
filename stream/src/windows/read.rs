use crate::windows::OSError;
use std::os::windows::io::{IntoRawHandle, RawHandle};
use std::mem::{MaybeUninit};
use std::ffi::c_void;
use winbind::Windows::Win32::System::SystemServices::{OVERLAPPED,OVERLAPPED_0,OVERLAPPED_0_0};
use std::pin::Pin;
use std::task::{Poll};
use winbind::Windows::Win32::System::Diagnostics::Debug::GetLastError;
use winbind::Windows::Win32::Foundation::HANDLE;
use std::marker::PhantomPinned;
use winbind::Windows::Win32::System::Diagnostics::Debug::WIN32_ERROR;
use crate::windows::overlapped::{PayloadTrait, Parent};


///Reads from a file descriptor
pub struct Read {
    fd: RawHandle
}

#[derive(Debug)]
pub struct ReadBuffer(Vec<u8>);

impl ReadBuffer {
    pub fn into_contiguous(self) -> ContiguousReadBuffer {
        ContiguousReadBuffer(self.0)
    }
}

///Actually, both buffers are contiguous on Windows, but we support the same API for compatibiltiy.
pub struct ContiguousReadBuffer(Vec<u8>);
impl ContiguousReadBuffer {
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}



pub struct OSReadOptions;
impl OSReadOptions {
    pub fn new() -> Self {
        OSReadOptions
    }
}

struct ReadChild {
    buffer: ReadBuffer,
    //inner buffer ptr, overlapped are passed to OS.  So we pin this struct.
    _pinned: PhantomPinned,
}
impl PayloadTrait for ReadChild {
    type Ok = ReadBuffer;
    type Failure = (OSError,ReadBuffer);
    #[allow(non_snake_case)]
    fn begin_op(self: Pin<&mut Self>, handle: HANDLE, overlapped: Pin<&mut MaybeUninit<OVERLAPPED>>, hEvent: HANDLE, completion: unsafe extern "system" fn(u32, u32, *mut OVERLAPPED)) -> Poll<Result<Self::Ok,Self::Failure>> {
       self.read_impl(handle, overlapped, hEvent, completion)
    }
    #[allow(non_snake_case)]
    fn resume_op(self: Pin<&mut Self>, error_code: u32, bytes_transferred: u32, handle: HANDLE,overlapped: Pin<&mut OVERLAPPED>, hEvent: HANDLE, completion: unsafe extern "system" fn(u32, u32, *mut OVERLAPPED)) -> Poll<Result<Self::Ok, Self::Failure>> {
        if error_code != 0 {
            //todo: avoid this clone?
            return Poll::Ready(Err((OSError(WIN32_ERROR(error_code)), ReadBuffer(self.buffer.0.clone()))));
        }
        unsafe {
            let s = self.get_unchecked_mut();
            let new_len = s.buffer.0.len() + bytes_transferred as usize;
            s.buffer.0.set_len(new_len);
            let s = Pin::new_unchecked(s);
            s.read_impl(handle, overlapped.map_unchecked_mut(|e| std::mem::transmute(e)),hEvent, completion)
        }
    }
}
impl ReadChild {
    #[allow(non_snake_case)]
    fn read_impl(self: Pin<&mut Self>, handle: HANDLE, overlapped: Pin<&mut MaybeUninit<OVERLAPPED>>, hEvent: HANDLE, completion: unsafe extern "system" fn(u32, u32, *mut OVERLAPPED)) -> Poll<Result<<ReadChild as PayloadTrait>::Ok,<Self as PayloadTrait>::Failure>> {
        let as_mut = unsafe { self.get_unchecked_mut() };

        as_mut.buffer.0.reserve(READ_SIZE);
        unsafe {
            let overlapped_mut =  overlapped.get_unchecked_mut().assume_init_mut();
            *overlapped_mut = OVERLAPPED {
                //When the request is issued, the system sets this member
                Internal: MaybeUninit::uninit().assume_init(),
                //The system sets this member
                InternalHigh: MaybeUninit::uninit().assume_init(),
                Anonymous: OVERLAPPED_0 {
                    Anonymous: OVERLAPPED_0_0 {
                        /*The Offset and OffsetHigh members together represent a 64-bit file position. It is a byte offset from the start of the file or file-like device,
                        and it is specified by the user; the system will not modify these values. */
                        Offset:as_mut.buffer.0.len() as u32,
                        OffsetHigh: 0,
                    },
                },
                hEvent,
            };
            use winbind::Windows::Win32::Storage::FileSystem::ReadFileEx;
            //note that on windows, this will return on the current thread, therefore we don't have to deal with Send etc
            let next_write = as_mut.buffer.0.as_mut_ptr().add(as_mut.buffer.0.len());

            let result = ReadFileEx(handle, next_write as *mut c_void, READ_SIZE as u32, overlapped_mut, Some(completion));
            if result.0 != 0 {
                //everything is fine
                Poll::Pending
            }
            else {
                //todo: avoid this clone?
                Poll::Ready(Err((OSError(GetLastError()),ReadBuffer(as_mut.buffer.0.clone()))))
            }
        }
    }
}


const READ_SIZE: usize = 255; //not sure what value to pick here


impl Read {
    pub fn new<H: IntoRawHandle>(handle: H) -> Self {
        Self {
            fd: handle.into_raw_handle()
        }
    }

    ///Reads the entire fd into memory
    pub async fn all<'a, O: Into<OSReadOptions>>(&self, _os_read_options: O) -> Result<ReadBuffer,(OSError,ReadBuffer)> {
        let as_handle =  winbind::Windows::Win32::Foundation::HANDLE(self.fd as isize);
            let fut = Parent::new(as_handle, ReadChild {
                buffer: ReadBuffer(Vec::with_capacity(READ_SIZE)),
                _pinned: Default::default()
            });

            let result = fut.await;
            use winbind::Windows::Win32::System::Diagnostics::Debug::ERROR_BROKEN_PIPE;
            match result {
                //this is really a success message in this context, basically EOF
                Err((OSError(ERROR_BROKEN_PIPE),new_buffer)) => {
                    return Ok(new_buffer);
                }
                other => {
                    return other;
                }
            }

    }
}

#[cfg(test)] mod tests {
    use std::process::{Command, Stdio};
    use crate::windows::read::{Read, OSReadOptions};

    #[test] fn read_process() {
        let c = Command::new("systeminfo").stdout(Stdio::piped()).spawn().unwrap();
        let read = Read::new(c.stdout.unwrap());
        let future = read.all(OSReadOptions);
        let result = kiruna::test::test_await(future, std::time::Duration::from_secs(10)).unwrap();
        println!("result length {:?}",result.0.len());
        println!("result {:?}",result.0);
    }
}