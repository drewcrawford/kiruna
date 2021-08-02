use std::future::{Future};
use std::sync::mpsc::{Receiver, channel, Sender};
use crate::sync_task::{SyncTask,ChannelType};

pub struct SyncExecutor {
    channel: Receiver<ChannelType>,
    spawner: Sender<ChannelType>,
    num_spawned: u16
}

impl SyncExecutor {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        SyncExecutor {
            channel: receiver,
            spawner: sender,
            num_spawned: 0
        }
    }
    pub fn spawn(&mut self, future: impl Future<Output=()>+ 'static) {
        self.num_spawned += 1;
        let task = SyncTask::new(future, self.spawner.clone());
        task.begin();
    }
    pub fn drain(mut self) {
        while let Ok(wakeup) = self.channel.recv() {
            //should be safe because it's only called from this thread
            let result = unsafe{ wakeup.poll() };
            if result.is_ready() {
                self.num_spawned -= 1;
            }
            if self.num_spawned == 0 { break }
        }
    }
}