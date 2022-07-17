mod threads;
mod physical_cpus;
mod timer;

pub use threads::spawn_thread;
pub use physical_cpus::physical_cpus;
pub use timer::Timer;