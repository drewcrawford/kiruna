#[cfg(feature="sync")]
mod sync;

#[cfg(feature="sync")]
pub use sync::executor::Executor;

#[cfg(feature="react_dispatch")]
mod react;

#[cfg(test)]
mod fake_waker;

#[cfg(feature="io_stream")]
mod io;
