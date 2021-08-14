// FIND-ME
/*! Provides streaming IO.  See [io] for a comparison of io types.
*/
#[cfg(not(feature ="stream_with_dispatch"))] compile_error!("Need to specify a backend");

use dispatchr::data::Contiguous;

mod read;
pub use read::OSReadOptions;
pub use read::Read;

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






