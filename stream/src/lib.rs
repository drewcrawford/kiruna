// FIND-ME
/*! Provides streaming IO.  See `kiruna::io` for a comparison of io types.*/

use dispatchr::data::{ Managed, Unmanaged, DispatchData};

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
pub use write::OSWriteOptions;
use std::fmt::Formatter;
use priority::Priority;
use dispatchr::qos::QoS;

///Buffer type.
///
/// This is an opaque buffer managed by kiruna.
#[derive(Debug)]
pub struct Buffer(Managed);
impl Buffer {
    pub fn as_dispatch_data(&self) -> &dispatchr::data::Unmanaged {
        self.0.as_unmanaged()
    }
    pub fn as_contiguous(&self) -> Contiguous {
        Contiguous(dispatchr::data::Contiguous::new(self.as_dispatch_data()))
    }
    pub(crate) fn add(&mut self, tail: &Unmanaged) {
        self.0 = self.0.as_unmanaged().concat(tail)
    }
}

pub struct Contiguous(dispatchr::data::Contiguous);
impl Contiguous {
    pub fn as_dispatch_data(&self) -> &dispatchr::data::Unmanaged {
        self.0.as_dispatch_data()
    }
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}

trait PriorityDispatch {
    fn as_qos(&self) -> QoS;
}


impl PriorityDispatch for Priority {
    fn as_qos(&self) -> QoS {
        match self {
            Priority::UserWaiting => {QoS::UserInitiated}
            Priority::Testing => {QoS::Default}
            _ => {QoS::Unspecified}
        }
    }
}


