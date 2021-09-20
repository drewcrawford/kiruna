use crate::OSError;
use winbind::Windows::Win32::Foundation::HANDLE;

pub struct Write {
    fd: HANDLE
}
///Backend-specific write options.
///
pub struct OSWriteOptions {
}

impl Write {
    ///A fast path to write static data.
    pub async fn write_static<O: Into<OSWriteOptions>>(&self, buffer: &'static [u8], write_options: O) -> Result<(),OSError>{
        todo!();
    }
}