use dispatchr::time::Time;

#[derive(Clone)]
pub struct Semaphore {
    semaphore: dispatchr::semaphore::Managed,
}
impl Semaphore {
    pub fn new() -> Self {
        Self {
            semaphore: dispatchr::semaphore::Managed::new(0),
        }
    }
    pub fn signal(&self) {
        self.semaphore.signal();
    }

    pub fn wait_forever(&self) {
        self.semaphore.wait(Time::FOREVER);
    }


    pub fn decrement_immediate(&self) -> Result<(),()> {
        match self.semaphore.wait(Time::NOW) {
            0 => Ok(()),
            _ => Err(()),
        }
    }
}