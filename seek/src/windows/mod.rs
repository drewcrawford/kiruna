/*! The 'seek' personality.  This personality is appropriate for IO on objects that support a 'seek' operation, such as files.

The usecase imagined here is
1) Where the number of syscalls will be small.  For example, reading an entire file, or reading "a small number" of file segments.
2) Where the data read will be short-lived.  For example, you intend to convert the data to another format immediately.

For other cases, consider memory-mapping the file.

# Implementation status
This personality is implemented only on Windows.  All APIs should be considered unstable.

*/
mod async_future;
mod ibuffer;

use std::borrow::Cow;
use std::path::Path;
use windows::Storage::StorageFile;
use pcore::string::IntoParameterString;
use pcore::release_pool::{ReleasePool};
use std::mem::MaybeUninit;
use crate::windows::async_future::AsyncFuture;
use std::fmt::Formatter;
use windows::Storage::Streams::{IBuffer, InputStreamOptions};
use windows::Storage::Streams::Buffer as WinBuffer;

#[derive(thiserror::Error,Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("Windows error {0}")]
    WindowsCore(#[from] WindowsCoreError),
    #[error("Error during async operation {0}")]
    Async(#[from] async_future::Error),
}
//additional trampoline through WindowsCoreError
impl From<windows::core::Error> for Error {
    fn from(e: windows::core::Error) -> Self {
        Self::WindowsCore(e.into())
    }
}

///Read result.  Wraps ibuffer, an implementation detail that wraps windows::IBuffer
#[derive(Debug)]
pub struct Buffer(ibuffer::Buffer);
impl Buffer {
    fn new(ibuffer: IBuffer) -> Self {
        Self(ibuffer::Buffer::new(ibuffer))
    }
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}

//todo: maybe move to pcore?
/**
This interface in unstable.
*/
#[derive(Debug)]
pub struct WindowsCoreError(windows::core::Error);
impl std::fmt::Display for WindowsCoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for WindowsCoreError {}
impl From<windows::core::Error> for WindowsCoreError {
    fn from(e: windows::core::Error) -> Self {
        Self(e)
    }
}

pub struct Read;
impl Read {
    /**
    This windows-only API is unstable.

    Asynchronous read; reads the entire contents of a file.
    */
    pub async fn all(path: &Path, release_pool: &ReleasePool) -> Result<Buffer,Error> {
        let absolute_path;
        if path.is_relative() {
            let mut _absolute_path = std::env::current_dir().unwrap();
            _absolute_path.push(path);
            absolute_path = Cow::Owned(_absolute_path);
        }
        else {
            absolute_path = Cow::Borrowed(path);
        }

        let forward_slash_path = absolute_path.as_os_str().to_str().unwrap().replace('/',"\\");
        let mut header = MaybeUninit::uninit();
        let param = forward_slash_path.into_parameter_string(release_pool);
        let path_param = unsafe{param.into_hstring_trampoline(&mut header)};
        let storage_file = StorageFile::GetFileFromPathAsync(&path_param)?;
        let future = AsyncFuture::new(storage_file);
        let storage_file = future.await?;
        let properties_future = AsyncFuture::new(storage_file.GetBasicPropertiesAsync()?);
        let input_stream_future = AsyncFuture::new(storage_file.OpenSequentialReadAsync()?);
        let (properties,input_stream) = kiruna_join::try_join2(properties_future, input_stream_future).await.map_err(|e| e.merge())?;

        let capacity = properties.Size().unwrap();
        let capacity_u32 = capacity as u32;
        let buffer = WinBuffer::Create(capacity_u32)?;

        let read_operation = input_stream.ReadAsync(buffer,capacity_u32,InputStreamOptions::default())?;
        let read_buffer = AsyncFuture::new(read_operation).await?;
        let public_buffer = Buffer::new(read_buffer);
        Ok(public_buffer)
    }
}
#[test] fn test_read() {
    pcore::release_pool::autoreleasepool(|pool| {
        use std::path::PathBuf;
        //for whatever reason, this is a "relative" path, but it includes the current directory??
        let path = Path::new(file!());
        let components_iter = path.components();
        let path = components_iter.skip(1).fold(PathBuf::new(), |mut a,b| {a.push(b); a});

        let r = Read::all(&path, pool);
        let buffer = kiruna::test::test_await(r, std::time::Duration::from_secs(1)).unwrap();
        let slice = buffer.as_slice();
        //uh...
        let slice2 = "The call is coming from INSIDE the building.".as_bytes();
        assert!(slice.windows(slice2.len()).any(|w| w==slice2))
    })

}
