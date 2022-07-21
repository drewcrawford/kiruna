use std::alloc::{Layout};
use std::collections::HashSet;
use std::mem::MaybeUninit;
use windows::Win32::System::SystemInformation::{CPU_SET_INFORMATION_TYPE, GetSystemCpuSetInformation, SYSTEM_CPU_SET_INFORMATION};
use windows::Win32::System::Threading::GetCurrentProcess;

pub fn threadpool_size() -> u16 {
    /*
  A pseudo handle is a special constant, currently (HANDLE)-1, that is interpreted as the current process handle.
  For compatibility with future operating systems, it is best to call GetCurrentProcess
  instead of hard-coding this constant value.
   */
    let handle = unsafe{GetCurrentProcess()};

    //Get total number (size) of elements in the datastructure
    let mut returned_len = MaybeUninit::uninit();
    unsafe { GetSystemCpuSetInformation(std::ptr::null_mut(), 0, returned_len.assume_init_mut(), handle, 0); }

    //so that size is specified in bytes, but it's a little bit unclear to me how to satisfy the alignment requirements
    //of the type, if any.  My guess is microsoft doesn't require any alignment, but I'm not so confident that rust is
    //that lax.
    let _returned_len = unsafe{returned_len.assume_init()};
    let alloc_layout = Layout::from_size_align(_returned_len as usize, std::mem::align_of::<SYSTEM_CPU_SET_INFORMATION>()).unwrap();
    let alloc = unsafe { std::alloc::alloc(alloc_layout) };


    let r = unsafe { GetSystemCpuSetInformation(alloc as *mut _, _returned_len, returned_len.assume_init_mut(), handle, 0) };
    assert!(r.as_bool()); //docs suggest this cannot fail.

    let mut byte_offset = 0;
    let mut unique_cores = HashSet::new();
    let mut logical_cores = 0;
    while byte_offset < _returned_len {
        unsafe {
            let alloc_plus_bytes = alloc.add(byte_offset as usize);
            let element_head = alloc_plus_bytes as *const _ as *const SYSTEM_CPU_SET_INFORMATION;
            //Applications should skip any structures with unrecognized types.
            if (*element_head).Type == CPU_SET_INFORMATION_TYPE(0) {
                /*
                The way intel works is it returns multiple "logical cores" (that is, threads) with the same core index.
                The core index is often nonconsecutive (for example you will see 0,0,2,2,4,4,6,6 for 4-core,8-thread systems.
                So we just count the unique CoreIndexes.
                 */
                let core_index = (*element_head).Anonymous.CpuSet.CoreIndex;
                unique_cores.insert(core_index);
                logical_cores += 1;
            }
            //When iterating over this structure, use the size field to determine the offset to the next structure.
            byte_offset += (*element_head).Size;
        }

    }
    unsafe{std::alloc::dealloc(alloc, alloc_layout)}
    let cores: u16 = unique_cores.len().try_into().unwrap();
    //for reasons I don't understand, this performs well on alder lake.
    //neither the physical nor logical core counts perform as well.
    (cores + logical_cores) / 2

}

#[test]
fn test_threadpool_size() {
  let cpus = threadpool_size();
    println!("{cpus} threadpool size");
}