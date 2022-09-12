
macro_rules! format_story {
    ($($arg:tt)*) => ({
        #[cfg(feature="thread_stories")] {
            format!($($arg)*)
        }
        #[cfg(not(feature="thread_stories"))] {
            String::new()
        }
    })
}
pub(crate) use format_story;
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
    #[inline(always)] pub fn log(&self, _string: &str) {
        #[cfg(feature="thread_stories")] {
            use std::time::Instant;
            let id = &self.id;
            let instant = Instant::now();
            println!("{instant:?}:{id}: {_string}");
        }
    }
}