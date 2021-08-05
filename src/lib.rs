#[cfg(feature="sync")]
mod sync;

#[cfg(feature="sync")]
pub use sync::executor::Executor;

#[cfg(feature="react_kqueue_available")]
mod react;

#[cfg(test)]
mod fake_waker;
