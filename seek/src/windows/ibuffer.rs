use std::fmt::Formatter;
use windows::Storage::Streams::IBuffer;
use windows::Win32::System::WinRT::IBufferByteAccess;
use windows::core::Interface;
pub struct Buffer(IBuffer);
unsafe impl Send for Buffer {}
impl std::fmt::Debug for Buffer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("Buffer(length={})",self.0.Length().unwrap()))
    }
}
impl Buffer {
    pub fn as_slice(&self) -> &[u8] {
        let length = self.0.Length().unwrap().try_into().unwrap();
        unsafe {
            let byte_access: IBufferByteAccess = self.0.cast().unwrap();
            let ptr = byte_access.Buffer().unwrap();
            std::slice::from_raw_parts(ptr,length)
        }
    }
    pub fn new(b: IBuffer) -> Self {
        Buffer(b)
    }
}