/*!
A custom channel with lower latency than crossbeam_channel.
 */
use std::sync::Arc;
use crate::platform::Semaphore;

struct SharedOne<Element> {
    queue: crossbeam_queue::ArrayQueue<Element>,
    local_semaphore: Semaphore,
}
impl<Element> SharedOne<Element> {
    fn send(&self, element: Element) {
        let q = self.queue.push(element);
        assert!(q.is_ok());
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
        for level in 0..levels {
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
            match s.queue.pop() {
                Some(e) => return e,
                None => (),
            }
        }
        panic!("No element found");
    }
}
