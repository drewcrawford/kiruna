use std::os::raw::c_void;
use std::time::Duration;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::Threading::{CloseThreadpoolTimer, CreateThreadpoolTimer, SetThreadpoolTimer, TP_CALLBACK_ENVIRON_V3, TP_CALLBACK_INSTANCE, TP_CALLBACK_PRIORITY_LOW, TP_TIMER};
pub struct Timer {
    timer: *mut TP_TIMER
}
unsafe impl Sync for Timer {}
unsafe impl Send for Timer {}
impl Timer {
    pub fn new(call_me: extern "system" fn(a: *mut c_void, b: *mut c_void, c: *mut c_void), interval: Duration, leeway: Duration) -> Self {
        /*
        Neither InitializeThreadpoolEnvironment (winbase.h) nor TpInitializeCallbackEnviron (winnt.h)
        are available upstream in the windows-rs crate.  I should probably report that to them
        but upstream is hostile to my patches and I am avoidant *so*...

        This implementation is adapted from winnt.h.  It includes the following warning:

            // Do not manipulate this structure directly!  Allocate space for it
            // and use the inline interfaces below.

        So consider yourself warned I guess.
         */
        let mut environment = TP_CALLBACK_ENVIRON_V3::default();
        environment.Version = 3;
        //we want low priority for this timer.
        environment.CallbackPriority = TP_CALLBACK_PRIORITY_LOW;
        environment.Size = std::mem::size_of::<TP_CALLBACK_ENVIRON_V3>().try_into().unwrap();
        //cast fn pointer to the type windows-rs actually expects
        let cast_fn:  extern "system" fn (a: *mut TP_CALLBACK_INSTANCE, b: *mut c_void, c: *mut TP_TIMER) = unsafe{std::mem::transmute(call_me)};
        let timer = unsafe{CreateThreadpoolTimer(Some(cast_fn), Some(call_me as *const c_void as *mut c_void), Some(&environment))};
        assert!(timer != std::ptr::null_mut());

        let mut due_time = FILETIME::default();
        //windows uses 100-ns intervals for filetime
        let ns_100: u64 = (interval.as_nanos() / 100).try_into().unwrap();
        due_time.dwLowDateTime = (ns_100 & 0xffffffff) as u32;
        due_time.dwHighDateTime = ((ns_100 & 0xffffffff00000000) >> 32) as u32;
        unsafe{SetThreadpoolTimer(timer, Some(&due_time), interval.as_millis().try_into().unwrap(), leeway.as_millis().try_into().unwrap())};
        Self { timer }
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        unsafe{CloseThreadpoolTimer(self.timer)}
    }
}

#[test] fn make_timer() {
    let (sender, receiver) = std::sync::mpsc::channel();
    use std::mem::MaybeUninit;
    static mut SENDER: MaybeUninit<std::sync::mpsc::Sender<()>> = MaybeUninit::uninit();
    unsafe{SENDER = MaybeUninit::new(sender)};
    extern "system" fn my_timer_fn(_a: *mut c_void, _b: *mut c_void, _c: *mut c_void) {
        unsafe {
            SENDER.assume_init_ref().send(()).unwrap();
        }

    }
    let _f = Timer::new(my_timer_fn, Duration::from_nanos(0), Duration::from_nanos(0));
    receiver.recv_timeout(std::time::Duration::from_millis(100)).unwrap();
}