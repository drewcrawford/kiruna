// FIND-ME
/*! Provides streaming IO.  See `kiruna::io` for a comparison of io types.*/

pub mod read;
pub mod write;

///An os-specific error type
#[derive(Debug)]
pub struct OSError(pub(crate) i32);
impl std::fmt::Display for OSError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("<OSError {}>",self.0))
    }
}
impl std::error::Error for OSError {

}


pub use read::{OSOptions, ContiguousBuffer, ReadBuffer, Read};
pub use write::Write;
pub use write::OSOptions;
use std::fmt::Formatter;
use priority::Priority;
use dispatchr::qos::QoS;



pub(super) trait PriorityDispatch {
    ///Gets the QoS for the specified priority
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



