use crate::OSError;
use winbind::Windows::Win32::Foundation::HANDLE;
use crate::windows::overlapped::{PayloadTrait, Parent};
use std::pin::Pin;
use std::mem::MaybeUninit;
use std::task::Poll;
use winbind::Windows::Win32::System::SystemServices::{OVERLAPPED,OVERLAPPED_0,OVERLAPPED_0_0};
use winbind::Windows::Win32::Storage::FileSystem::WriteFileEx;
use winbind::Windows::Win32::System::Diagnostics::Debug::{GetLastError,WIN32_ERROR};
use std::ffi::c_void;
use std::marker::PhantomData;
use std::future::Future;

pub struct Write {
    fd: HANDLE
}
///Backend-specific write options.
///
pub struct OSOptions<'a>(&'a PhantomData<()>);
impl<'a> OSOptions<'a> {
    pub fn new() -> Self {
        OSOptions(&PhantomData)
    }
}

trait AsSlice {
    fn as_slice(&self) -> &[u8];
}

struct StaticSlice(&'static [u8]);
impl AsSlice for StaticSlice {
    fn as_slice(&self) -> &[u8] {
        self.0
    }
}

struct WriteOp<Slice> {
    buffer: Slice,
}
impl<Slice: AsSlice> PayloadTrait for WriteOp<Slice> {
    type Ok = ();
    type Failure = OSError;

    fn begin_op(self: Pin<&mut Self>, handle: HANDLE, overlapped: Pin<&mut MaybeUninit<OVERLAPPED>>, h_event: HANDLE, completion: unsafe extern "system" fn(u32, u32, *mut OVERLAPPED)) -> Poll<Result<Self::Ok, Self::Failure>> {
        unsafe {
            let overlapped_mut = overlapped.get_unchecked_mut().assume_init_mut();
            *overlapped_mut = OVERLAPPED {
                //When the request is issued, the system sets this member
                Internal: MaybeUninit::uninit().assume_init(),
                //The system sets this member
                InternalHigh: MaybeUninit::uninit().assume_init(),
                Anonymous: OVERLAPPED_0 {
                    Anonymous: OVERLAPPED_0_0 {
                        /*For files that support byte offsets, you must specify a byte offset at which to start writing to the file. You specify this offset by setting the Offset and OffsetHigh members of the OVERLAPPED structure.
                        For files or devices that do not support byte offsets, Offset and OffsetHigh are ignored. */
                        Offset:0,
                        OffsetHigh: 0,
                    },
                },
                hEvent: h_event,
            };
            let slice = self.buffer.as_slice();
            let r = WriteFileEx(handle,slice.as_ptr() as *const c_void,slice.len() as u32, overlapped_mut, Some(completion));
            if r.0 == 0 {
                Poll::Ready(Err(OSError(GetLastError())))
            }
            else {
                Poll::Pending
            }
        }

    }
    fn resume_op(self: Pin<&mut Self>, error_code: u32, _bytes_transferred: u32, _handle: HANDLE, _overlapped: Pin<&mut OVERLAPPED>, _h_event: HANDLE, _completion: unsafe extern "system" fn(u32, u32, *mut OVERLAPPED)) -> Poll<Result<Self::Ok, Self::Failure>> {
        if error_code != 0 {
            Poll::Ready(Err(OSError(WIN32_ERROR(error_code))))
        }
        else {
            Poll::Ready(Ok(()))
        }
    }
}

impl Write {
    ///New write operation
    pub fn new(handle: std::os::windows::prelude::RawHandle) -> Self {
        Write {
            fd: HANDLE(handle as isize)
        }
    }
    ///A fast path to write static data.
    pub fn write_static<'a, O: Into<OSOptions<'a>>>(&self, buffer: &'static [u8], _write_options: O) -> impl Future<Output=Result<(),OSError>> + 'a {
        let fut = Parent::new(self.fd, WriteOp{
            buffer: StaticSlice(buffer)
        });
        async {
            fut.await
        }
    }
}

#[cfg(test)] mod test {
    use std::process::{Command, Stdio};
    use crate::windows::write::{Write};
    use crate::read::{Read};
    use std::os::windows::io::AsRawHandle;

    #[test] fn sort() {
        let mut c = Command::new("sort").stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        {
            let write_to = c.stdin.take().unwrap();
            let handle = write_to.as_raw_handle();
            let w = Write::new(handle);
            let fut = w.write_static("a\r\nb\r\nd\r\nc\r\n".as_bytes(), crate::write::OSOptions::new());
            let r = kiruna::test::test_await(fut, std::time::Duration::from_secs(10));
            r.unwrap();
        }
        let read = Read::new(c.stdout.unwrap());
        let read_fut = read.all(crate::read::OSOptions::new());
        let r = kiruna::test::test_await(read_fut, std::time::Duration::from_secs(10));
        let contiguous = r.unwrap().into_contiguous();
        let slice = contiguous.as_slice();
        let str = std::str::from_utf8(slice).unwrap();
        println!("{}",str);
        assert_eq!(str,"a\r\nb\r\nc\r\nd\r\n")
    }
}