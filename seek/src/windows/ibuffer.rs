use windows::Storage::Streams::IBuffer;
use windows::Win32::System::WinRT::IBufferByteAccess;
use windows::core::Interface;
#[derive(Debug)]
pub struct Buffer(IBuffer);
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