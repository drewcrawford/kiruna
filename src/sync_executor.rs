use std::future::{Future};
use std::sync::mpsc::{Receiver, channel, Sender};
use crate::sync_task::{SyncTask,ChannelType};

pub struct SyncExecutor {
    channel: Receiver<ChannelType>,
    spawner: Option<Sender<ChannelType>>
}

impl SyncExecutor {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        SyncExecutor {
            channel: receiver,
            spawner: Some(sender)
        }
    }
    pub fn spawn(&self, future: impl Future<Output=()>+ 'static) {
        let task = SyncTask::new(future, self.spawner.as_ref().expect("Executor already drained").clone());
        task.begin();
    }
    pub fn drain(mut self) {
        drop(self.spawner.take().expect("Spawner already drained"));
        while let Ok(wakeup) = self.channel.recv() {
            //should be safe because it's only called from this thread
            unsafe{ wakeup.poll() };
        }
    }
}