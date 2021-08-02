use std::future::{Future};
use std::sync::mpsc::{Receiver, channel, Sender};
use crate::sync_task::{SyncTask, SyncWake, ChannelFactory};
use std::task::{Waker, Context};
use std::sync::Arc;

pub struct SyncExecutor {
    channel: Receiver<SyncExecutorHandle>,
    spawner: ChannelFactory,
    local_spawner: Sender<SyncExecutorHandle>,
    tasks: Vec<Option<SyncTask>>,
    num_active: u16,
}

#[derive(Copy,Clone)]
pub(crate) struct SyncExecutorHandle(u16);

impl SyncExecutor {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        let local_spawner = sender.clone();
        let factory = unsafe{ ChannelFactory::new(sender)};
        SyncExecutor {
            channel: receiver,
            spawner: factory,
            local_spawner,
            tasks: Vec::new(),
            num_active: 0
        }
    }
    pub fn spawn(&mut self, future: impl Future<Output=()>+ 'static) {
        let new_handle = SyncExecutorHandle(self.tasks.len() as u16);
        let task = SyncTask::new(future);
        self.tasks.push(Some(task));
        self.num_active += 1;
        self.local_spawner.send(new_handle).unwrap();
    }
    pub fn drain(mut self) {
        while let Ok(wakeup) = self.channel.recv() {
            let task = self.tasks[wakeup.0 as usize].as_ref().unwrap();
            let sync_wake = SyncWake::new(wakeup, self.spawner.clone());
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