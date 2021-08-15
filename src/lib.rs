#[cfg(feature="sync")]
mod sync;

#[cfg(feature="sync")]
pub use sync::executor::Executor;


#[cfg(any(test,feature="test"))]
pub mod test;

#[cfg(feature="io_stream")]
pub mod io;
mod priority;
#[cfg(feature="futures")]
pub mod futures;

pub use priority::Priority;
