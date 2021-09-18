#[cfg(feature="sync")]
mod sync;

#[cfg(feature="sync")]
pub use sync::executor::Executor;


#[cfg(any(test,feature="test"))]
pub mod test;

pub mod io;

#[cfg(feature="futures")]
pub mod futures;
#[cfg(feature="join")]
pub mod join;

pub use priority::Priority;
