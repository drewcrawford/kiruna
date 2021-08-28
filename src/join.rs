use std::pin::Pin;
use std::task::{Context, Poll};
use std::future::Future;
use std::fmt::{Formatter};
use std::error::Error;

enum ReadyOrNot<F, O>
{
    Future(F),
    Ready(O),
}

enum Join2<A,B,AR,BR> where A: Future<Output=AR>,B:Future<Output=BR> {
    Polling {
        a: ReadyOrNot<A,AR>,
        b: ReadyOrNot<B,BR>
    },
    Done
}
impl <A,B,AR,BR> Join2<A,B,AR,BR> where A: Future<Output=AR>,B:Future<Output=BR>  {
    fn new(a: A, b: B) -> Self {
        Self::Polling {
            a: ReadyOrNot::Future(a),
            b: ReadyOrNot::Future(b)
        }
    }
}

impl<A,B,AR,BR> std::future::Future for Join2<A,B,AR,BR> where A: std::future::Future<Output=AR>, B: std::future::Future<Output=BR> {
    type Output = (AR,BR);

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {

        let this = unsafe{ self.get_unchecked_mut() }; //fuck you borrowchecker
        let (a,b) = match this {
            Self::Polling {a,b} => (a,b),
            Self::Done => { panic!("Polled after completion")}
        };
        if let ReadyOrNot::Future(f) = a {
            if let Poll::Ready(r) = unsafe { Pin::new_unchecked(f).poll(cx)} {
                *a = ReadyOrNot::Ready(r)
            }
        }
        if let ReadyOrNot::Future(f) = b {
            if let Poll::Ready(r) = unsafe { Pin::new_unchecked(f).poll(cx)} {
                *b = ReadyOrNot::Ready(r)
            }
        }
        match (a, b) {
            (ReadyOrNot::Ready(_), ReadyOrNot::Ready(_)) =>
            //move Done into this, returning previous this
                match std::mem::replace(this, Self::Done) {
                    Self::Polling {
                        //collect owned references
                        a: ReadyOrNot::Ready(a),
                        b: ReadyOrNot::Ready(b),
                    } => Poll::Ready((a,b)),
                    //we already checked this case in the match
                    _ => unreachable!(),
                },
            //not yet ready
            _ => Poll::Pending,
        }
    }
}

pub fn join2<A,B>(a: A, b: B) -> impl Future<Output=(A::Output,B::Output)> where A: Future, B: Future{
    Join2::new(a,b)
}

#[derive(Debug)]
pub enum Error2<A: std::error::Error,B: std::error::Error> {
    A(A),
    B(B)
}
impl<A: std::error::Error,B:std::error::Error> std::fmt::Display for Error2<A,B> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Error2::A(a) => f.write_fmt(format_args!("A: {}",a)),
            Error2::B(b) => {f.write_fmt(format_args!("B: {}",b))}
        }
    }
}
impl<A: std::error::Error,B:std::error::Error> std::error::Error for Error2<A,B> {}
///Implementations where both errors are the same type
impl<A: std::error::Error> Error2<A,A> {
    ///If the two errors are the same, merge them into one
    pub fn merge(self) -> A {
        match self {
            Error2::A(a) => {a}
            Error2::B(a) => {a}
        }
    }
}


enum TryJoin2<A,B,AR,BR,AE,BE> where A: Future<Output=Result<AR,AE>>,B:Future<Output=Result<BR,BE>> {
    Polling {
        a: ReadyOrNot<A,AR>,
        b: ReadyOrNot<B,BR>
    },
    Done
}
impl<A,B,AR,BR,AE,BE> TryJoin2<A,B,AR,BR,AE,BE> where A: Future<Output=Result<AR,AE>>,B:Future<Output=Result<BR,BE>> {
    pub fn new(a: A, b: B) -> Self {
        Self::Polling {
            a: ReadyOrNot::Future(a),
            b: ReadyOrNot::Future(b)
        }
    }
}

impl<A,B,AR,BR,AE,BE> Future for TryJoin2<A,B,AR,BR,AE,BE> where A: Future<Output=Result<AR,AE>>,B:Future<Output=Result<BR,BE>>,BE:Error, AE: Error {
    type Output = Result<(AR,BR),Error2<AE,BE>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = unsafe{ self.get_unchecked_mut() }; //fuck you borrowchecker
        let (a,b) = match this {
            Self::Polling {a,b} => (a,b),
            Self::Done => { panic!("Polled after completion")}
        };
        if let ReadyOrNot::Future(f) = a {
            match unsafe { Pin::new_unchecked(f).poll(cx)} {
                Poll::Ready(Ok(item)) => {
                    *a = ReadyOrNot::Ready(item)
                }
                Poll::Ready(Err(e)) => {
                    return Poll::Ready(Err(Error2::A(e)))
                }
                Poll::Pending => {}
            }
        }
        if let ReadyOrNot::Future(f) = b {
            match unsafe { Pin::new_unchecked(f).poll(cx)} {
                Poll::Ready(Ok(item)) => {
                    *b = ReadyOrNot::Ready(item)
                }
                Poll::Ready(Err(e)) => {
                    return Poll::Ready(Err(Error2::B(e)))
                }
                Poll::Pending => {}
            }
        }
        match (a, b) {
            (ReadyOrNot::Ready(_), ReadyOrNot::Ready(_)) =>
            //move Done into this, returning previous this
                match std::mem::replace(this, Self::Done) {
                    Self::Polling {
                        //collect owned references
                        a: ReadyOrNot::Ready(a),
                        b: ReadyOrNot::Ready(b),
                    } => Poll::Ready(Ok((a,b))),
                    //we already checked this case in the match
                    _ => unreachable!(),
                },
            //not yet ready
            _ => Poll::Pending,
        }
    }
}

///A 'fail-fast' future joiner
pub fn try_join2<A,B,AR,BR,AE,BE>(a: A, b: B) -> impl Future<Output=Result<(AR,BR),Error2<AE,BE>>>where A:Future<Output=Result<AR,AE>>,B:Future<Output=Result<BR,BE>>,
AE:std::error::Error,BE:std::error::Error
{
    TryJoin2::new(a,b)
}
