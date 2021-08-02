mod sync_task;
mod sync_executor;

pub use sync_executor::SyncExecutor;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
