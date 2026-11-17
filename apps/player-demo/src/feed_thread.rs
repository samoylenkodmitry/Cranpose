// A thread that feeds a decoder from its own loop, until the handle drops.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

// The handle of a feed thread. The thread reads the flag between its pictures and ends once the
// handle has dropped.
pub struct FeedThread {
    stop: Arc<AtomicBool>,
}

impl FeedThread {
    // Starts a thread named `name` that runs `feed`, which returns once the flag is set.
    pub fn spawn(name: &str, feed: impl FnOnce(&AtomicBool) + Send + 'static) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let spawned = thread::Builder::new()
            .name(name.into())
            .spawn(move || feed(&flag));
        if let Err(error) = spawned {
            log::error!("player demo: no {name} thread: {error}");
        }
        Self { stop }
    }
}

impl Drop for FeedThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
