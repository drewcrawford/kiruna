use std::os::raw::c_void;
use windows::Win32::System::Threading::{CreateThread, ResumeThread, SetThreadPriority, THREAD_CREATION_FLAGS, THREAD_PRIORITY_ABOVE_NORMAL};
use crate::bin::WhichBin;

extern "system" fn routine(param: *mut c_void) -> u32 {
    let ptr: fn() = unsafe{std::mem::transmute(param)};
    ptr();
    0
}

pub fn spawn_thread<F: FnOnce() + Send + 'static>(priority: priority::Priority,_micro_priority: MicroPriority,  f: F) {
    let stack_size_bytes = 128*1000; //dunno?
    let f_as_cvoid = f as *const c_void;
    let thread_suspended = THREAD_CREATION_FLAGS(0x00000004);
    let handle = unsafe{CreateThread(std::ptr::null_mut(), stack_size_bytes, Some(routine), f_as_cvoid, thread_suspended, std::ptr::null_mut())}.unwrap();
    let priority = match priority {
        Priority::UserWaiting | Priority::Testing =>  THREAD_PRIORITY_ABOVE_NORMAL,
    };
    //todo: _micro_priority is currently unused on this platform.
    unsafe{SetThreadPriority(handle, priority).unwrap()};
    let r = unsafe{ResumeThread(handle)};
    assert!(r != u32::MAX);
}