use std::ffi::c_void;
use std::time::Duration;
use dispatchr::QoS;
use dispatchr::source::Managed;
use dispatchr::time::Time;

pub struct Timer {
    source: dispatchr::source::Managed,
}
impl Timer {
    pub fn new(call_me: extern "C" fn(*mut c_void), interval: Duration, leeway: Duration) -> Self {
        let global_queue = dispatchr::queue::global(QoS::Background).unwrap();
        let source = Managed::create(dispatchr::source::dispatch_source_type_t::timer(), 0, 0, global_queue);
        source.set_timer(Time::NOW, interval.as_nanos().try_into().unwrap(), leeway.as_nanos().try_into().unwrap());
        source.set_event_handler_f(call_me);
        source.resume();
        Self {
            source,
        }
    }
}