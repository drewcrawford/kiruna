use std::os::raw::c_void;
use windows::core::PCWSTR;
use windows::Win32::System::Threading::{CreateThread, ResumeThread, SetThreadDescription, SetThreadPriority, THREAD_CREATION_FLAGS, THREAD_PRIORITY_ABOVE_NORMAL};
use priority::Priority;
use crate::MicroPriority;

extern "system" fn routine<F: FnOnce() + Send + 'static>(param: *mut c_void) -> u32 {
    let unboxed_f: Box<F> = unsafe{Box::from_raw(param as *mut F)};
    unboxed_f();
    0
}

pub fn spawn_thread<F: FnOnce() + Send + 'static>(priority: Priority,_micro_priority: MicroPriority, debug_name: &str, f: F) {
    let stack_size_bytes = 128*1000; //dunno?
    let boxed_fn = Box::new(f);
    let boxed_raw = Box::into_raw(boxed_fn);
    let thread_suspended = THREAD_CREATION_FLAGS(0x00000004);
    let handle = unsafe{CreateThread(std::ptr::null_mut(), stack_size_bytes, Some(routine::<F>), boxed_raw as *const c_void, thread_suspended, std::ptr::null_mut())}.unwrap();
    let priority = match priority {
        Priority::UserWaiting | Priority::Testing =>  THREAD_PRIORITY_ABOVE_NORMAL,
        _ => todo!()
    };
    //todo: _micro_priority is currently unused on this platform.
    unsafe{SetThreadPriority(handle, priority).unwrap()};
    let c_name = widestring::U16CString::from_str(debug_name).unwrap();
    unsafe{SetThreadDescription(handle, PCWSTR(c_name.as_ptr())).unwrap()};
    let r = unsafe{ResumeThread(handle)};
    assert!(r != u32::MAX);
}