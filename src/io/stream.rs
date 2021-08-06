/*! Provides streaming IO.  See [io] for a comparison of io types.
*/

mod read_op;

use std::os::unix::io::{RawFd, IntoRawFd};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use crate::react::Reactor;
use libc::read;

struct Stream {
    fd: RawFd
}


impl Stream {
    fn new<T: IntoRawFd>(fd: T) -> Stream {
        Stream {
            fd: fd.into_raw_fd()
        }
    }

}