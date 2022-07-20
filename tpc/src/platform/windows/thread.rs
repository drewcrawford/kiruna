use crate::bin::WhichBin;

pub fn spawn_thread<F: FnOnce() + Send + 'static>(bin: WhichBin, f: F) {
    todo!()
}