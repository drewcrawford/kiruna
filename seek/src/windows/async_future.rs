/*! Implements Rust future on windows types

todo: move this to some common location? */

use windows::Foundation::{IAsyncOperation, AsyncOperationCompletedHandler, AsyncStatus, IAsyncOperationWithProgress, AsyncOperationWithProgressCompletedHandler};
use windows::core::{RuntimeType};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::sync::{Mutex, Arc};
/**
Erase the underlying windows type.  This may be, for example, [IAsyncOperation], [IAsyncOperationWithProgress], among others.
*/
pub trait WindowsOperation<A: RuntimeType + 'static>: Sized {
    fn set_completed<F: FnMut(&Self,AsyncStatus) -> windows::core::Result<()> + 'static>(&self, handler: F) -> windows::core::Result<()>;
    fn get(&self) -> windows::core::Result<A>;
}
impl<A: RuntimeType> WindowsOperation<A> for IAsyncOperation<A> {
    fn set_completed<F: FnMut(&Self,AsyncStatus) -> windows::core::Result<()> + 'static>(&self, mut handler: F) -> windows::core::Result<()> {
        let h = AsyncOperationCompletedHandler::new(move |a,b| {
            let s3lf = a.as_ref().unwrap();
            handler(s3lf,b)
        });
        Self::SetCompleted(self,h)
    }
    fn get(&self) -> windows::core::Result<A> {
        self.get()
    }
}
impl<A: RuntimeType,P: RuntimeType> WindowsOperation<A> for IAsyncOperationWithProgress<A,P> {
    fn set_completed<F: FnMut(&Self,AsyncStatus) -> windows::core::Result<()> + 'static>(&self, mut handler: F) -> windows::core::Result<()> {
        let h = AsyncOperationWithProgressCompletedHandler::new(move |a,b| {
            let s3lf = a.as_ref().unwrap();
            handler(s3lf,b)
        });
        Self::SetCompleted(self,h)
    }
    fn get(&self) -> windows::core::Result<A> {
        self.get()
    }
}
#[derive(thiserror::Error,Debug)]
pub enum Error {
    #[error("The operation was cancelled.")]
    Cancelled,
    #[error("{0}")]
    Error(windows::core::Error),
}
pub enum State<Output> {
    Initial,
    SetCompleted(Waker),
    Done(Result<Output,Error>),
    Gone,
    InternalError,
}

///Wraps some IAsyncOperation and implements [std::future::Future].
pub struct AsyncFuture<Output: RuntimeType + 'static,Operation: WindowsOperation<Output>> {
    operation: Operation,
    state: Arc<Mutex<State<Output>>>,
}
impl<Output: RuntimeType + 'static,Operation: WindowsOperation<Output>> AsyncFuture<Output,Operation> {
    pub fn new(operation: Operation) -> Self {
        Self{operation: operation,state: Arc::new(Mutex::new(State::Initial))}
    }
}
impl<Output: RuntimeType + 'static,Operation: WindowsOperation<Output>> Future for AsyncFuture<Output,Operation> {
    type Output = Result<Output,Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        //Result is not Unpin here
        let s = unsafe { self.get_unchecked_mut()};
        let move_mutex = s.state.clone();
        let mut state_guard = s.state.lock().unwrap();
        let mut relocate = State::InternalError;
        //SET *state_guard IN ALL PATHS THROUGH THE FN!
        std::mem::swap(&mut *state_guard,&mut relocate);
        match relocate {
            State::Initial => {
                *state_guard = State::SetCompleted(cx.waker().clone());
                let completion_handler = move |result: &Operation,status| {
                    /*
                    It is kinda unclear to me what is going on here.  Docs suggest `status` can take on values like Started, so I guess we expect this function to be called multiple times, potentially.
                     */
                    let set_output = match status {
                        AsyncStatus::Error => {
                            Result::Err(Error::Error(result.get().err().unwrap()))
                        }
                        AsyncStatus::Canceled => {
                            Result::Err(Error::Cancelled)
                        }
                        AsyncStatus::Completed => {
                            let results = result.get();
                            match results {
                                Ok(item) => Result::Ok(item),
                                Err(err) => Result::Err(Error::Error(err))
                            }
                        }
                        _ => {
                            todo!()
                        }
                    };
                    let mut tmp = State::Done(set_output);
                    let mut state_guard = move_mutex.lock().unwrap();

                    std::mem::swap(&mut tmp, &mut*state_guard);
                    match tmp {
                        State::Initial => unreachable!(),
                        State::SetCompleted(waker) => {
                            waker.wake_by_ref();
                        },
                        State::Done(_) => unreachable!(),
                        State::Gone => unreachable!(),
                        State::InternalError => unreachable!(),
                    }
                    Ok(())
                };
                s.operation.set_completed(completion_handler).unwrap();
                Poll::Pending
            }
            State::SetCompleted(_) => {
                //update waker
                *state_guard = State::SetCompleted(cx.waker().clone());
                Poll::Pending
            }
            State::Done(done) => {
                *state_guard = State::Gone;
                Poll::Ready(done)
            }
            State::Gone => {
                panic!("Already gone!")
            },
            State::InternalError => unreachable!()
        }

    }
}