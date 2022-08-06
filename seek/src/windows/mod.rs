/*! The 'seek' personality.  This personality is appropriate for IO on objects that support a 'seek' operation, such as files.

The usecase imagined here is
1) Where the number of syscalls will be small.  For example, reading an entire file, or reading "a small number" of file segments.
2) Where the data read will be short-lived.  For example, you intend to convert the data to another format immediately.

For other cases, consider memory-mapping the file.

# Implementation status
This personality is implemented only on Windows.  All APIs should be considered unstable.

*/
mod ibuffer;

use std::ffi::OsString;
use std::path::{Path};
use windows::Storage::StorageFile;
use pcore::string::IntoParameterString;
use pcore::release_pool::{ReleasePool};
use std::mem::MaybeUninit;
use std::fmt::Formatter;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use windows::core::InParam;
use windows::Storage::Streams::{IBuffer, InputStreamOptions};
use windows::Storage::Streams::Buffer as WinBuffer;

#[derive(thiserror::Error,Debug)]
#[non_exhaustive]
pub enum Error {
    #[error("Windows error {0}")]
    WindowsCore(#[from] WindowsCoreError),
    // #[error("Error during async operation {0}")]
    // Async(#[from] winfuture::Error),
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

fn fix_path(path: &Path) -> OsString {
    //todo: this could be substantially optimized to avoid allocations, return a borrowed value, use iterators, etc.
    /*
    There are many things we need to do to sanitize a path as appropriate for this function.
    First, we must use an absolute path.
     */
    let path = if path.is_relative() {
        let mut _absolute_path = std::env::current_dir().unwrap();
        _absolute_path.push(path);
        _absolute_path
    }
    else {
        path.to_owned() //todo: we could omit this copy
    };
    //second we want to replace all slashes with \.
    //this is documented in https://docs.microsoft.com/en-us/uwp/api/windows.storage.storagefile.getfilefrompathasync?view=winrt-22621
    let os_string = path.into_os_string();
    let wide = os_string.encode_wide().map(|char| if char == '/' as u16 { '\\' as u16 } else { char });

    //in some cases, the path contains the characters \\?\.  This has something to do with changing how windows processes paths, see
    //https://stackoverflow.com/questions/21194530/what-does-mean-when-prepended-to-a-file-path
    //In any case, the API doesn't like these paths.
    let mut collect: Vec<u16> = wide.collect();
    if collect.starts_with(&[92,92,63,92]) {
        collect.drain(0..4);
    }
    let path: OsString = OsStringExt::from_wide(&collect);
    path
}
pub struct Read;
impl Read {
    /**
    This windows-only API is unstable.

    Asynchronous read; reads the entire contents of a file.
    */
    pub async fn all(path: &Path, _priority: priority::Priority, _release_pool: &ReleasePool) -> Result<Buffer,Error> {
        let path = fix_path(path);

        let mut header = MaybeUninit::uninit();
        let path_param = unsafe{path.into_hstring_trampoline(&mut header)};
        let storage_file = StorageFile::GetFileFromPathAsync(&path_param)?.await?;
        let properties_future = storage_file.GetBasicPropertiesAsync()?;
        let input_stream_future = storage_file.OpenSequentialReadAsync()?;
        let (properties,input_stream) = kiruna_join::try_join2(properties_future, input_stream_future).await.map_err(|e| e.merge())?;

        let capacity = properties.Size().unwrap();
        let capacity_u32 = capacity as u32;
        let buffer = WinBuffer::Create(capacity_u32)?;
        let as_ibuffer: IBuffer = buffer.try_into().unwrap();
        let read_operation = input_stream.ReadAsync(InParam::borrowed(windows::core::Borrowed::new(Some(&as_ibuffer))),capacity_u32,InputStreamOptions::None)?;
        let read_buffer = read_operation.await?;
        let public_buffer = Buffer::new(read_buffer);
        Ok(public_buffer)
    }
}
#[test] fn test_fix_path() {
    //check a variety of path bugs
    use std::path::PathBuf;
    use std::str::FromStr;
    let relative = fix_path(&PathBuf::from_str("my_relative_path").unwrap()).into_string().unwrap();
    let chars: Vec<char> = relative.chars().collect();
    assert_eq!(chars[1], ':');
    assert_eq!(chars[2], '\\');

    let slash = fix_path(&PathBuf::from_str("C:\\test/this\\path\\").unwrap()).into_string().unwrap();
    assert_eq!(&slash, "C:\\test\\this\\path\\");

    let weird_prefix = fix_path(&PathBuf::from_str("\\\\?\\C:\\").unwrap()).into_string().unwrap();
    assert_eq!(&weird_prefix, "C:\\");
}
