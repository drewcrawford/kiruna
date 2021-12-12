/*!
A personality based on async reads/writes.  This is generally preferred where

1.  Allocations are short-lived (e.g., you're not going to hang onto the memory region very long or at all)
2.  We may be able to work ahead on the thread while waiting for the load
3.  The total number of calls is going to be small, like 1.  e.g., read the entire file.
 */

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub use crate::windows::Read;