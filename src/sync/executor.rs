use std::future::{Future};
use std::sync::mpsc::{Receiver, channel, Sender};
use crate::sync::task::{Task, Wake, ChannelFactory};
use std::task::{Waker, Context};
use std::sync::Arc;

///Main executor
pub struct Executor {
    channel: Receiver<Handle>,
    spawner: ChannelFactory,
    local_spawner: Sender<Handle>,
    tasks: Vec<Option<Task>>,
    num_active: u16,
}

#[derive(Copy,Clone)]
pub(crate) struct Handle(u16);

impl Executor {
    ///Creates a new executor
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        let local_spawner = sender.clone();
        let factory = unsafe{ ChannelFactory::new(sender)};
        Executor {
            channel: receiver,
            spawner: factory,
            local_spawner,
            tasks: Vec::new(),
            num_active: 0
        }
    }
    ///Spawns the task onto the executor.
    pub fn spawn(&mut self, future: impl Future<Output=()>+ 'static) {
        let new_handle = Handle(self.tasks.len() as u16);
        let task = Task::new(future);
        self.tasks.push(Some(task));
        self.num_active += 1;
        self.local_spawner.send(new_handle).unwrap();
    }
    ///Drains the executor.  After this call, the executor can no longer be used.
    ///
    /// This function will return when all spawned tasks complete.
    pub fn drain(mut self) {
        while let Ok(wakeup) = self.channel.recv() {
            let task = self.tasks[wakeup.0 as usize].as_ref().unwrap();
            let sync_wake = Wake::new(wakeup, self.spawner.clone());
            let as_waker: Waker = Arc::new(sync_wake).into();
            let mut context = Context::from_waker(&as_waker);
            //safe because it's always called from this thread
            let result = unsafe{ task.poll(&mut context) };
            if result.is_ready() {
                self.num_active -= 1;
                self.tasks[wakeup.0 as usize].take();
            }
            if self.num_active == 0 { break }
        }
    }
}