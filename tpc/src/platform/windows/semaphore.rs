use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::System::Threading::{CreateSemaphoreW, ReleaseSemaphore, WaitForSingleObject};

pub struct Semaphore {
    semaphore: HANDLE,
}
impl Semaphore {
    pub fn new() -> Self {
        let s = unsafe{CreateSemaphoreW(None, 0, i32::MAX, None).unwrap()};
        Self {
            semaphore: s,
        }
    }
    pub fn signal(&self) {
        unsafe{ReleaseSemaphore(self.semaphore, 1, None).unwrap()};
    }
    pub fn decrement_immediate(&self) -> Result<(), ()> {
        unsafe {
            match WaitForSingleObject(self.semaphore, 0) {
                result if result == WAIT_OBJECT_0 => {
                    Ok(())
                }
                result if result == WAIT_TIMEOUT => {
                    Err(())
                }
                other => {
                    panic!("unexpected result from WaitForSingleObject: {:?}", other);
                }
            }
        }
    }
    pub fn wait_forever(&self) {
        unsafe {
            match WaitForSingleObject(self.semaphore, u32::MAX) {
                result if result == WAIT_OBJECT_0 => {
                    /* ok! */
                }
                other => {
                    panic!("unexpected result from WaitForSingleObject: {:?}", other);
                }
            }
        }
    }
}
impl Drop for Semaphore {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.semaphore);
        }
    }
}