/*for test purposes: the 62nd character in the file is here: abcdefgh  */
/*!
A personality based on async reads/writes.  This is generally preferred where

1.  Allocations are short-lived (e.g., you're not going to hang onto the memory region very long or at all)
2.  We may be able to work ahead on the thread while waiting for the load
3.  The total number of calls is going to be small, like 1.  e.g., read the entire file.
 */

pub struct Buffer(imp::Buffer);
impl Buffer {
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }
}

pub struct Read(imp::Read);
impl Read {
    /**
    Asynchronous read; reads the entire contents of a file.
     */
    pub fn all(path: &Path, priority: priority::Priority, release_pool: &ReleasePool) -> impl Future<Output=Result<Buffer,Error>> + Send {
        let fut = imp::Read::all(path, priority, release_pool);
        async {
            fut.await.map(|o| Buffer(o)).map_err(|e| Error(e))
        }
    }
    pub fn new(path: &Path, priority: Priority) -> impl Future<Output=Result<Self,Error>> + Send + '_ {
        async move {
            let r = imp::Read::new(path, priority).await;
            r.map(|o| Read(o)).map_err(|e| Error(e))
        }
    }
    pub fn read(&mut self, offset: usize, size: usize) -> impl Future<Output=Result<Buffer,Error>> + Send + '_ {
        let fut = self.0.read(offset, size);
        async {
            fut.await.map(|o| Buffer(o)).map_err(|e| Error(e))
        }
    }
}
#[derive(Debug,boil::Display,boil::Error)]
pub struct Error(imp::Error);
impl Error {

}

#[cfg(target_os = "windows")]
mod windows;

use std::future::Future;
use std::path::Path;
use pcore::release_pool::ReleasePool;
use priority::Priority;
#[cfg(target_os = "windows")]
use crate::windows as imp;

#[cfg(target_os="macos")]
mod macos;
#[cfg(target_os="macos")]
use crate::macos as imp;

#[test] fn test_read() {
    pcore::release_pool::autoreleasepool(|pool| {
        use std::path::PathBuf;
        //for whatever reason, this is a "relative" path, but it includes the current directory??
        let path = Path::new(file!());
        let components_iter = path.components();
        let path = components_iter.skip(1).fold(PathBuf::new(), |mut a,b| {a.push(b); a});

        let r = Read::all(&path, kiruna::Priority::Testing,pool);
        let buffer = kiruna::test::test_await(r, std::time::Duration::from_secs(10)).unwrap();
        let slice = buffer.as_slice();
        //uh...
        let slice2 = "The call is coming from INSIDE the building.".as_bytes();
        assert!(slice.windows(slice2.len()).any(|w| w==slice2))
    })
}

#[test] fn read_offset() {
    use std::path::PathBuf;
    //for whatever reason, this is a "relative" path, but it includes the current directory??
    let path = Path::new(file!());
    let components_iter = path.components();
    let path = components_iter.skip(1).fold(PathBuf::new(), |mut a,b| {a.push(b); a});

    let file_fut = Read::new(&path, Priority::Testing);
    let mut r = kiruna::test::test_await(file_fut, std::time::Duration::from_secs(1)).unwrap();
    let read_fut = r.read(61, 8);
    let buffer = kiruna::test::test_await(read_fut, std::time::Duration::from_secs(10)).unwrap();
    let slice = buffer.as_slice();
    //uh...
    let slice2 = "abcdefgh".as_bytes();
    assert!(slice.windows(slice2.len()).any(|w| w==slice2))
}

#[test] fn stress_test() {
    for _ in 0..50 {
        pcore::release_pool::autoreleasepool(|pool| {
            use std::path::PathBuf;
            //for whatever reason, this is a "relative" path, but it includes the current directory??
            let path = Path::new(file!());
            let components_iter = path.components();
            let path = components_iter.skip(1).fold(PathBuf::new(), |mut a,b| {a.push(b); a});
            let r = Read::all(&path, kiruna::Priority::Testing,pool);
            kiruna::test::test_await(r, std::time::Duration::from_secs(10)).unwrap();
        });
    }


}