use std::os::raw::{c_char, c_int};
use std::ffi::c_void;
use std::mem::MaybeUninit;

extern "C" {
    fn sysctlbyname(name: *const c_char, oldp: *mut c_void, oldlenp: *mut c_void, newp: *mut c_void, newlen: usize) -> c_int;
    fn __errno_location() -> *mut c_int;
}
pub fn physical_cpus() -> u16 {
    unsafe {
        let mut r: MaybeUninit<u32> = std::mem::MaybeUninit::uninit();
        let mut r_size = std::mem::size_of::<u32>();
        const NAME: [u8; 15] = *b"hw.physicalcpu\0";
        let sysctl_result = sysctlbyname(&NAME as *const _ as *const c_char, &mut r as *mut _ as *mut c_void, &mut r_size as *mut _ as *mut c_void, std::ptr::null_mut(), 0);
        if sysctl_result != 0 {
            panic!("Can't get CPU size sysctl_result {sysctl_result} oldlenp {r_size}");
        }
        r.assume_init() as u16
    }

}

#[test] fn test_physical_cpus() {
    println!("{} physical cpus",physical_cpus());

}