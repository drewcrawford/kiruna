// FIND-ME
/*! Provides streaming IO.  See [io] for a comparison of io types.*/
#[cfg(not(feature ="stream_with_dispatch"))] compile_error!("Need to specify a backend");

use dispatchr::data::Contiguous;

mod read;
mod write;

///An os-specific error type
#[derive(Debug)]
pub struct OSError(i32);
impl std::fmt::Display for OSError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("<OSError {}>",self.0))
    }
}
impl std::error::Error for OSError {

}


pub use read::OSReadOptions;
pub use read::Read;
pub use write::Write;
use std::fmt::Formatter;

///Buffer type.
///
/// This is an opaque buffer managed by kiruna.
#[derive(Debug)]
pub struct Buffer(Contiguous);
impl Buffer {
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
    pub fn as_dispatch_data(&self) -> &dispatchr::data::Unmanaged {
        self.0.as_dispatch_data()
    }
}






