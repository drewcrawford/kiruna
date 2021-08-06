use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use libc::read;
use std::os::unix::io::RawFd;
use crate::react::Reactor;
use std::rc::Rc;

#[derive(Debug,Clone)]
struct ResizableBuffer(Rc<Vec<u8>>);

///This will perform a single read operation, as suggested
/// by the reactor.  This is not so useful in itself, but is simpler
/// than working with the reactor directly.
#[derive(Debug)]
struct ReadOp {
    fd: RawFd,
    resizable_buffer: ResizableBuffer
}
#[derive(Debug)]
enum ReadResult {
    Read(ResizableBuffer),
    EOF(ResizableBuffer),
    Error(ResizableBuffer)
}
impl ReadResult {
    fn is_read(&self) -> bool {
        match self {
            ReadResult::Read(_) => {true }
            ReadResult::EOF(_) => {false}
            ReadResult::Error(_) => {false}
        }
    }
    fn is_eof(&self) -> bool {
        match self {
            ReadResult::Read(_) => {false}
            ReadResult::EOF(_) => {true}
            ReadResult::Error(_) => {false}
        }
    }
    fn into_buffer(self) -> ResizableBuffer {
        match self {
            ReadResult::Read(b) => {b}
            ReadResult::EOF(b) => {b}
            ReadResult::Error(b) => {b}
        }
    }

}
impl Future for ReadOp {

    type Output = ReadResult;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<ReadResult> {
        //println!("poll");
        let self_own = self.get_mut();
        let buffer_mut = Rc::get_mut(&mut self_own.resizable_buffer.0).unwrap();
        let ready = Reactor::poll(self_own.fd, cx.waker().clone());
        if let Poll::Ready(ready) = ready {
            let u_ready = ready.size() as usize;

            buffer_mut.reserve(u_ready);
            let old_len = buffer_mut.len();
            let raw_ptr = buffer_mut.as_mut_ptr();
            let write_ptr = unsafe{ raw_ptr.add(old_len) }; //write past the end of the buffer
            //blocking read
            let read_result = unsafe{ read(self_own.fd, std::mem::transmute(write_ptr), u_ready) };
            //println!("Read result {:?}",read_result);
            if read_result >= 0 {
                let actual_len = old_len + read_result as usize;
                unsafe{ buffer_mut.set_len(actual_len) }
                //println!("new data {:?}",buffer_mut.len());
                let new_buffer = self_own.resizable_buffer.clone();
                if ready.is_eof() {
                    Poll::Ready(ReadResult::EOF(new_buffer))
                }
                else {
                    Poll::Ready(ReadResult::Read(new_buffer))
                }
            }
            else {
                let new_buffer = self_own.resizable_buffer.clone();
                Poll::Ready(ReadResult::Error(new_buffer))
            }
        }
        else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod test {
    use std::os::unix::io::RawFd;
    use libc::pipe;
    pub fn test_pipe() -> (RawFd, RawFd) {
        let mut pipes = [0,0];
        let pipe_result = unsafe{ pipe(&mut pipes as *mut _)};
        assert_eq!(pipe_result,0);
        (pipes[0],pipes[1])
    }
}

#[test] fn read_op() {
    use std::path::Path;
    use std::time::Duration;
    use libc::{write,close};
    use crate::fake_waker::{toy_poll, toy_await};

    use std::thread::sleep;
    use std::os::unix::io::IntoRawFd;
    let (fd,write_fd) = test::test_pipe();

    //write 10mb to pipe
    std::thread::spawn(move || {
        let mut buf: Vec<u8> = Vec::with_capacity(1000);
        let mut n = 0;
        //
        for _ in 0..1000_0 {
            buf.push(n);
            n = n.wrapping_add(1);
        }
        for _ in 0..100_0 {
            let write_result = unsafe{ write(write_fd, std::mem::transmute(buf.as_ptr()), buf.len()) };
            assert!(write_result > 0);
        }
        let close_result = unsafe{ close(write_fd)} ;
        assert!(close_result == 0);
    });
    let instant = std::time::Instant::now();

    //create one buffer that we can use for unlimited reads
    let buffer = ResizableBuffer(Rc::new(Vec::new()));
    let op = ReadOp {
        fd,
        resizable_buffer: buffer
    };

    let output = toy_await(op, Duration::from_secs(1));
    //println!("output {:?}",output.is_eof());
    assert!(output.is_read());
    let buffer = output.into_buffer();
    assert!(buffer.0.len() > 0);
    assert_eq!(buffer.0[0], 0);
    assert_eq!(buffer.0[1], 1);
    assert_eq!(buffer.0[2], 2);

    let new_op = ReadOp {
        fd,
        resizable_buffer: buffer
    };
    let mut new_output = toy_await(new_op,Duration::from_secs(1));
    assert!(new_output.is_read());

    //keep going until we get eof
    let mut rounds = 2;
    while !new_output.is_eof() {

        let new_op = ReadOp {
            resizable_buffer: new_output.into_buffer(),
            fd: fd
        };
        new_output = toy_await(new_op, Duration::from_secs(1));
        rounds += 1;
    }
    println!("Did {:?} rounds",rounds);
}