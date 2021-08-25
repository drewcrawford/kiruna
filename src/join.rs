use std::pin::Pin;
use std::task::{Context, Poll};
use std::future::Future;

enum ReadyOrNot<F, O>
    where
        F: Future<Output = O>,
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