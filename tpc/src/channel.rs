/*!
A custom channel with lower latency than crossbeam_channel.

# History

A brief discussion of the history of this channel.

Originally, I somewhat reluctantly used crossbeam-channel.  For reasons in the next section, we use their queue, so why not the whole channel?

In fact, it is very good for high-throughput situations.  But in cases where you have low throughput and are worried about latency,
the `park` implementation is quite slow, I think it implements its own exponential backoff which is not what we want.

It is an interesting question how "important" of a problem this is - for example, maybe we want to push low throughput into a different
executor like kiruna::Sync.  On some level, a threadpool that moves futures across threads is always going to be slower than a threadpool
that is not.  However, crossbeam-channel is somewhat egregious and I believe this case can at least be improved.

I also took a look at Flume.  In fact it solves the low-threadpool case very well.  However on the vec benchmarks it is significantly slower
than crossbeam-channel.  I believe this is due to its use of internal locks, which crossbeam can avoid via its lock-free queue.

So I decided to write my own channel, encoding some of the peculariaties of kiruna::tpc.  In particular, we have a hierarchical channel,
the number of levels of which are known at build-time, rather than APIs like `select` that wait for a message to arrive on any channel
from a set of channels known at runtime.

This approach benchmarks interchangeably with crossbeam-channel on high-throughput, and is substantially better at low-latency.
So.  I think it is a good choice for kiruna::tpc.

# queue
A brief discussion of the dependency on crossbeam_queue.

I looked into writing my own atomic queue.  I even designed one that seems in a cursory examination to be better
than the widely-used michael-scott scheme (it is easier to analyze at least, which makes it a good fit for kiruna's goals).

It seems that algorithms in the class, including my design, need some actual non-arc GC.  There's a window of time
where a reference count has reached zero and some other thread is trying to acquire it; to solve this you have to defer
the deallocation 'awhile', or have some scheme where you tag pointers in the unused bits to fit them in a word,
and decide not to reference them because they have old tags, etc.

I think writing a garbage collector is a little outside my scope at present, and the best queue based on an existing gc is going to
be the crossbeam queue.  I think it is not totally optimal on weakly-ordered memory machines but.


 */
use crate::platform::Semaphore;

struct SharedOne<Element> {
    queue: crossbeam_queue::ArrayQueue<Element>,
    local_semaphore: Semaphore,
}
impl<Element> SharedOne<Element> {
    fn send(&self, element: Element) {
        let q = self.queue.push(element);
        assert!(q.is_ok()); //we cannot unwrap because element is not necessary Debug
        self.local_semaphore.signal();
    }
    fn recv_nonblock(&self) -> Option<Element> {
        match self.local_semaphore.decrement_immediate() {
            Ok(()) => {
                Some(self.queue.pop().unwrap())
            }
            Err(()) => {
                None
            }
        }
    }

}


pub struct Channel<Element> {
    shared: Vec<SharedOne<Element>>,
    global_semaphore: Semaphore,
}
impl<Element> Channel<Element> {
    pub fn new(bounded: usize, levels: usize) -> Self {
        let mut shared = Vec::with_capacity(levels);
        let global_semaphore = Semaphore::new();
        for _ in 0..levels {
            let s = SharedOne {
                queue: crossbeam_queue::ArrayQueue::new(bounded),
                local_semaphore:Semaphore::new(),
            };
            shared.push(s);
        }
        Self {
            shared,
            global_semaphore,
        }
    }
    pub fn send(&self, message: Element, level: usize) {
        self.shared[level].send(message);
        self.global_semaphore.signal();
    }
    pub fn recv_immediate(&self, level: usize) -> Option<Element> {
        match self.global_semaphore.decrement_immediate() {
            Ok(()) => {
                //see if it's on this level or not
                match self.shared[level].recv_nonblock() {
                    Some(item) => {
                        Some(item)
                    }
                    None => {
                        //it wasn't on this level, so we need to restore our semaphore
                        self.global_semaphore.signal();
                        None
                    }
                }
            }
            Err(()) => {
                //no items available
                None
            }
        }
    }
    pub fn recv_all(&self) -> Element {
        self.global_semaphore.wait_forever();
        for s in &self.shared {
            match s.recv_nonblock() {
                Some(e) => return e,
                None => (),
            }
        }
        panic!("No element found");
    }
}
