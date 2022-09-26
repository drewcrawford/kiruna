/*!Implements a threadpool on windows to serve completion ports.

 At the time of this writing, this is implemented with a single thread that performs
 overlapped I/O.

 */

#[derive(Copy,Clone)]
pub struct TaskHandle;
use windows::Win32::Foundation::HANDLE;
use once_cell::sync::OnceCell;
use std::sync::mpsc::{Receiver, sync_channel, SyncSender};
use windows::core::PCSTR;

struct WinSemaphore(HANDLE);
impl WinSemaphore {
    fn new() -> Self {
        use windows::Win32::System::Threading::CreateSemaphoreA;
        let handle = unsafe{ CreateSemaphoreA(None, 0, 2, PCSTR(std::ptr::null_mut()))};
        Self(handle.unwrap())
    }
}
impl Drop for WinSemaphore {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        let r = unsafe{ CloseHandle(self.0)};
        assert!(r.0 != 0)
    }
}
unsafe impl Sync for WinSemaphore {}

///Wrapper type 'upgrades' Send to Sync, for types that we only use from one thread
struct OneThread<T:Send>(T);
unsafe impl<T: Send> Sync for OneThread<T> {}
impl<T: Send> OneThread<T> {
    unsafe fn only_one_thread(&self) -> &T { &self.0 }
}

struct WorkerThread {
    semaphore: WinSemaphore,
    receiver: OneThread<Receiver<Box<dyn FnOnce(TaskHandle)-> () + 'static + Send>>>
}
impl WorkerThread {
    fn begin_thread() {
        std::thread::spawn(move || { threadpool().worker.in_thread() });
    }
    fn in_thread(&self) {
        loop {
            use windows::Win32::System::Threading::WaitForSingleObjectEx;
            //enter an alertable wait state
            let reason = unsafe{ WaitForSingleObjectEx(self.semaphore.0, u32::MAX, true)};
            use windows::Win32::Foundation::WAIT_OBJECT_0;
            if reason == WAIT_OBJECT_0 {
                //at this point, we have woken because some fn was scheduled
                //(there are other reasons we might wake, in particular, AIO completion)
                let r = unsafe{self.receiver.only_one_thread()}.recv().unwrap();
                r(TaskHandle);
                //back to our regularly-scheduled waiting

            }

        }
    }
}

struct Threadpool {
    //for right now, our threadpool is a single worker
    worker: WorkerThread,
    sender: SyncSender<Box<dyn FnOnce(TaskHandle)-> () + 'static + Send>>
}
impl Threadpool {
    fn send<F>(&self, f: F) where F: FnOnce(TaskHandle) -> () + 'static + Send {
        self.sender.send(Box::new(f)).unwrap();
        //evidently you 'release' a sempahore to signal it in win32
        use windows::Win32::System::Threading::ReleaseSemaphore;
        unsafe{ ReleaseSemaphore(self.worker.semaphore.0, 1, None)};
    }
}

static THREADPOOL: OnceCell<Threadpool> = OnceCell::new();

fn threadpool() -> &'static Threadpool {
    THREADPOOL.get_or_init(|| {
        let (sender,receiver) = sync_channel(5);
        let t = Threadpool {
            sender: sender,
            worker: WorkerThread {
            semaphore: WinSemaphore::new(),
                receiver: OneThread(receiver)
            }
        };
        WorkerThread::begin_thread();
        t
    })
}
/**Enqueues a task for execution on the threadpool.

The way this works is as follows:

1.  At some point, your function is executed, with an argument described below
2.  Later, the thread will weave in and out of alertable wait states
3.  Presumably, you called some function in 1 that will cause this thread to be woken up
4.  If you are no longer interested in the waits for some reason, you must call the function `remove_task`

Task here is a long-lived handle.  Although your closure only executes once, presumably you have some
way (in the alertable wait state) to wake and run more code.  The same task can be used until you
are done with the whole process.
*/
pub fn add_task<F>(f: F) where F: FnOnce(TaskHandle) -> () + Send + 'static {
    let pool = threadpool();
    pool.send(f);
}

///Remove the task.  Currently this has no effect, in the future it might enable the runtime
/// to make optimizations because your task is no longer running.
pub fn remove_task(_handle: TaskHandle) {
    //todo
}