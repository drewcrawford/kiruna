use std::sync::Arc;
use std::task::Wake;

//fake waker purely for internal purposes
pub(crate) struct FakeWaker;
impl Wake for FakeWaker {
    fn wake(self: Arc<Self>) {
        //nothing
    }
    fn wake_by_ref(self: &Arc<Self>) {
        //nothing either!
    }
}