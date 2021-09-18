///Models the priority of a task.
///
/// This normally ought to be set by the top level application,
/// as e.g. reading a lot file is a lot different than opening a user document,
/// and this is normally not possible to distinguish in a library.  Therefore,
/// libraries ought to be careful to expose the right information to their callers,
/// potentially a good way up the stack.
///
/// This type maps to some underlying OS-specific idea of thread or task priorities.
///
/// Priority can be converted into various other types like OSReadOptions or OSWriteOptions.
/// Doing so may lose a bit of control but is sufficient for most applications.
#[non_exhaustive]
#[derive(Copy,Clone)]
pub enum Priority {
    ///The user is actively blocked waiting for the result.  This is a high-priority task, but not realtime.
    UserWaiting,
    ///This priority is used for most unit tests that don't especially care what priority they use.
    Testing
}


