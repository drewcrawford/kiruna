use std::sync::mpsc::SyncSender;
use std::sync::Arc;
use crate::sync_task::{SyncTask};
use std::future::Future;
use std::rc::Rc;

struct SyncSpawner {
    channel: SyncSender<Rc<dyn Future<Output=()>>>
}
impl SyncSpawner {
    fn spawn(&self, future: impl Future<Output=()> + 'static) {
        let task = SyncTask::new(future, self.channel.clone());
    }
}