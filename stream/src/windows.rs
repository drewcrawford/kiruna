pub mod read;
mod threadpool;
pub mod write;
mod overlapped;

use windows::Win32::Foundation::WIN32_ERROR;
use std::fmt::Formatter;

#[derive(Debug)]
pub struct OSError(pub(super)WIN32_ERROR);

impl std::fmt::Display for OSError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        use windows::Win32::System::Diagnostics::Debug::{FormatMessageA,FORMAT_MESSAGE_FROM_SYSTEM,FORMAT_MESSAGE_ALLOCATE_BUFFER, FORMAT_MESSAGE_IGNORE_INSERTS};
        let mut buf: *mut u8 = std::ptr::null_mut();
        let buf_len = unsafe {
            FormatMessageA(
                FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_ALLOCATE_BUFFER | FORMAT_MESSAGE_IGNORE_INSERTS,
                None,
                self.0.0,
                //If you pass in zero, FormatMessage looks for a message for LANGIDs in the following order
                0,
                //signature wants a PSTR here but doc suggests we can pass in any pointer
                //PSTR ought to be repr-transparent, so...
                std::mem::transmute(&mut buf),
                // If FORMAT_MESSAGE_ALLOCATE_BUFFER is set, this parameter specifies the minimum number of TCHARs to allocate for an output buffer.
                0,
                None,
            )
        };
        let as_slice = unsafe{ std::slice::from_raw_parts(buf, buf_len as usize)};
        let as_str = std::str::from_utf8(as_slice).unwrap();
        let r = f.write_fmt(format_args!("{}",as_str));
        use windows::Win32::System::Memory::LocalFree;
        //should be fine because FORMAT_MESSAGE_ALLOCATE_BUFFER
        unsafe{ LocalFree(buf as isize)};
        r
    }
}

impl std::error::Error for OSError {

}


#[test] fn print_error() {
    use windows::Win32::Foundation::ERROR_BROKEN_PIPE;
    let e = OSError(ERROR_BROKEN_PIPE);
    println!("{}",e);
}