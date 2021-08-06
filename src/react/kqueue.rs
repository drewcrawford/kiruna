//! A kqueue implementation of a reactor.
use std::os::unix::io::RawFd;
use libc::{kqueue,kevent,EVFILT_READ,EV_ADD,EV_ONESHOT,EV_EOF,timespec,close};
use std::task::{Waker, Poll};
use std::collections::{HashMap, HashSet};
use std::sync::{MutexGuard, Mutex};
use once_cell::sync::Lazy;
use std::thread::sleep;
use std::fmt::Formatter;

///Debugging utilities
struct ProvidesDebug(kevent);
impl std::fmt::Debug for ProvidesDebug {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("ident: {}, flags: {},filter: {},data:{},udata:{:?}",self.0.ident,self.0.flags,self.0.filter,self.0.data,self.0.udata))
    }
}

#[derive(Debug)]
pub(crate) struct KqueueResult {
    size: isize,
    eof: bool
}
impl KqueueResult {
    fn from(event_list: &kevent) -> KqueueResult {
        KqueueResult {
            size: event_list.data,
            eof: (event_list.flags & EV_EOF) != 0
        }
    }
    pub fn size(&self) -> isize {
        self.size
    }
    pub fn is_eof(&self) -> bool {
        self.eof
    }
}

///High-speed kqueue poller.
///
/// This currently notifies about reads being available, although it could be extended to other events.
pub struct KqueueReactor {
    ///internal kqueue pointer
    kqueue: i32,
    //for now, we use a hashset here.  Not sure if this is strictly the most optimal choice
    //or if we should use kernel memory to handle our identifiers, but this is safer
    waiting: HashMap<RawFd,Waker>,
    ///The fd, mapped to the number of data available for reading
    ready: HashMap<RawFd,KqueueResult>,
    kqueue_thread: bool,
}
///Although not currently used, this cleans up the kqueue type if we ever support dropping it.
impl Drop for KqueueReactor {
    fn drop(&mut self) {
        assert!(!self.kqueue_thread);
        let result = unsafe{ close(self.kqueue)};
        assert_eq!(result,0);
    }
}

impl KqueueReactor {
    fn new() -> KqueueReactor {
        let kqueue = unsafe{ kqueue()};
        assert_ne!(kqueue,-1);

        KqueueReactor {
            kqueue,
            waiting: HashMap::new(),
            ready: HashMap::new(),
            kqueue_thread: false
        }
    }
    ///Gets access to the shared reactor, a singleton pattern.
    fn shared() -> MutexGuard<'static, KqueueReactor> {
        static REACTOR: Lazy<Mutex<KqueueReactor>> = Lazy::new(|| Mutex::new(KqueueReactor::new()));
        REACTOR.lock().unwrap()
    }

    ///Awakens the appropriate [kevent].
    ///
    /// This will insert the [kevent] into the ready queue  and remove from the waiting queue.
    fn awake(&mut self, event: kevent) {
        //println!("awake {:?}",event.ident);
        let result = KqueueResult::from(&event);
        self.ready.insert(event.ident as i32, result);
        /*
        Suppose we're in this config

        1.  worker thread got kevent
        2.  And is trying to get the lock to do awake
        3.  Lock is held by same element which is currently polling
            4.  Element inserted, set up oneshot
            5.  release lock from poll
        6.  Lock acquired on worker thread
        7.  Do wake and remove element
        8.  New wakeup due to oneshot in 4
            9.  Wake is no longer available

        For this reason, we allow failures on 9

         */
        self.waiting.remove(&(event.ident as i32)).map(|p| p.wake());
    }

    ///slow-speed poller, tried after the other versions fail.
    ///
    /// In any case, calling this function is an indication that the result is not ready.  If the
    /// reactor is not already running, this will spin up a working thread to handle the poll.
    fn poll_long(&mut self) {
        //only spin up worker thread if necessary
        if self.kqueue_thread { return }

        let kqueue_fd = self.kqueue;
        self.kqueue_thread = true;
        std::thread::spawn(move || {
            let mut eventlist = kevent {
                ident: 0,
                filter: 0,
                flags: 0,
                fflags: 0,
                data: 0,
                udata: std::ptr::null_mut()
            };
            loop {
                //println!("B:kevent({},<omitted>,0,<{:?}>,1,NULL)",kqueue_fd,ProvidesDebug(eventlist));
                let r = unsafe{kevent(kqueue_fd, std::ptr::null(), 0, &mut eventlist, 1, std::ptr::null())};
                assert_eq!(r,1);
                //println!("B:kevent returned {}, eventlist: {:?}",r,ProvidesDebug(eventlist));

                let mut lock = KqueueReactor::shared();
                //println!("hashset {:?}",lock.waiting.keys());
                lock.awake(eventlist);
            }


        });

    }

    ///A 'medium-speed' poll.
    ///
    /// This function does an 'inline' [kevent] which tries
    /// to service the poll on the current thread.  This often works, in which case
    /// it will return the result
    ///
    /// If this doesn't work, it will try other methods and return [Poll::Pending].
    fn poll_medium(&mut self, fd: RawFd, waker: Waker) -> Poll<KqueueResult> {
        //Assuming that doesn't work, we need to register on our kqueue.
        self.waiting.insert(fd,waker);
        //Both polling and adding can take place at the same time, so let's go ahead and poll inline here instantaneously
        //This is likely a fast path
        let u_fd = fd as usize;
        let changelist = kevent {
            ident: u_fd,
            filter: EVFILT_READ,
            flags: EV_ADD | EV_ONESHOT,
            fflags: 0,
            data: 0,
            udata: std::ptr::null_mut()
        };
        let mut eventlist = kevent {
            ident: 0,
            filter: 0,
            flags: 0,
            fflags: 0,
            data: 0,
            udata: std::ptr::null_mut()
        };
        let timeout = timespec {
            tv_sec: 0,
            tv_nsec: 0
        };

        let allow_medium_path = true;
        let nevents = if allow_medium_path { 1 } else { 0 };
        //println!("A:kevent({},<{:?}>,1,<omitted>,{},[0,0])",self.kqueue,ProvidesDebug(changelist),nevents);
        let kevent_result = unsafe{ kevent(self.kqueue, &changelist, 1, &mut eventlist, nevents, &timeout)};
        //println!("A:kevent returned {}, eventlist: <{:?}>",kevent_result,ProvidesDebug(eventlist));
        assert!(kevent_result>=0);
        if kevent_result == 1 {
            if eventlist.ident == u_fd{
                //this item is no longer waiting
                //println!("removing waiting");
                self.waiting.remove(&fd).unwrap();
                Poll::Ready(KqueueResult::from(&eventlist))
            }
            else {
                self.awake(eventlist);
                self.poll_long();
                Poll::Pending
            }
        }
        else {
            self.poll_long();
            Poll::Pending
        }

    }
    ///Top-level read poll for a file descriptor
    ///
    /// Polls the file descriptor for reading with a variety of strategies.
    /// This will yield the number of bytes available to read.
    ///
    /// In the case it returns [Poll::Pending], we will call the waker when data is available
    fn poll_fast(&mut self, fd: RawFd, waker: Waker) -> Poll<KqueueResult> {
        //first, determine if this is in the awoken set
        if let Some(data) = self.ready.remove(&fd) {
            Poll::Ready(data)
        }
        else {
            self.poll_medium(fd,waker)
        }
    }

    ///Convenience method for [poll_fast] that acquires a lock
    pub(crate) fn poll(fd: RawFd, waker: Waker) -> Poll<KqueueResult> {
        Self::shared().poll_fast(fd,waker)
    }
}

#[test] fn read_file() {
    use crate::fake_waker::FakeWaker;
    use std::os::unix::io::IntoRawFd;
    use std::sync::Arc;
    use std::path::Path;
    use std::time::Duration;
    let test_path = Path::new("src/react/kqueue.rs");
    let file = std::fs::File::open(test_path).unwrap();
    let fd = file.into_raw_fd();
    let instant = std::time::Instant::now();

    while instant.elapsed() < Duration::from_secs(2) {
        //println!("poll {:?}",instant.elapsed());
        let waker = FakeWaker::new_waker();
        let poll_result = KqueueReactor::shared().poll_fast(fd, waker);
        if poll_result.is_ready() {
            //println!("Ready {:?}",poll_result);
            sleep(Duration::from_secs(1));
            return;
        }
        else {
            //println!("poll");
        }
    }
    panic!("Future never arrived");

}