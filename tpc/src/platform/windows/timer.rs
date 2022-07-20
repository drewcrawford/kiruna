use std::os::raw::c_void;
use std::time::Duration;

pub struct Timer {
}
impl Timer {
    pub fn new(call_me: extern "C" fn(*mut c_void), interval: Duration, leeway: Duration) -> Self {
        todo!()
    }
}