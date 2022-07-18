
pub struct Story {
    #[cfg(feature="thread_stories")]
    id: String,
}
impl Story {
    pub fn new() -> Story {
        #[cfg(feature="thread_stories")]
        use rand::{Rng, SeedableRng};
        #[cfg(feature = "thread_stories")]
        use rand::distributions::Alphanumeric;
        Story {
            #[cfg(feature="thread_stories")]
            id: rand::rngs::StdRng::from_entropy().sample_iter(&Alphanumeric).take(5).map(char::from).collect()
        }
    }
    #[inline(always)] pub fn log(&self, string: String) {
        #[cfg(feature="thread_stories")] {
            use std::time::Instant;
            let id = &self.id;
            let instant = Instant::now();
            println!("{instant:?}:{id}: {string}");
        }
    }
}