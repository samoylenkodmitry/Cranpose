/// The frames a retained texture may go unused before it is released: long
/// enough to outlast a scroll back or a paused animation, short enough that
/// textures a scene has moved past -- another scale, content it no longer
/// shows, a burst of surfaces at startup -- do not hold memory until a
/// budget fills.
pub(crate) const IDLE_FRAMES: u64 = 120;

/// Items handed back for reuse, oldest first, each stamped with the frame
/// it came back in.
pub(crate) struct IdlePool<T> {
    available: Vec<(T, u64)>,
    frame: u64,
}

impl<T> Default for IdlePool<T> {
    fn default() -> Self {
        Self {
            available: Vec::new(),
            frame: 0,
        }
    }
}

impl<T> IdlePool<T> {
    /// Takes the oldest item `matches` accepts.
    pub(crate) fn take(&mut self, matches: impl Fn(&T) -> bool) -> Option<T> {
        let index = self.available.iter().position(|(item, _)| matches(item))?;
        Some(self.available.remove(index).0)
    }

    /// Hands `item` back, then drops the oldest items while more than
    /// `max_len` are held or, beyond the newest, they weigh more than
    /// `max_bytes`.
    pub(crate) fn put(
        &mut self,
        item: T,
        max_len: usize,
        max_bytes: u64,
        bytes: impl Fn(&T) -> u64,
    ) {
        self.available.push((item, self.frame));
        let mut held: u64 = self.iter().map(&bytes).sum();
        while self.available.len() > max_len || (held > max_bytes && self.available.len() > 1) {
            let (dropped, _) = self.available.remove(0);
            held = held.saturating_sub(bytes(&dropped));
        }
    }

    /// Ends a frame, dropping every item no frame has taken back out for
    /// [`IDLE_FRAMES`].
    pub(crate) fn end_frame(&mut self) {
        self.frame += 1;
        let oldest_kept = self.frame.saturating_sub(IDLE_FRAMES);
        self.available
            .retain(|(_, returned)| *returned >= oldest_kept);
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &T> {
        self.available.iter().map(|(item, _)| item)
    }

    pub(crate) fn len(&self) -> usize {
        self.available.len()
    }
}

#[cfg(test)]
#[path = "tests/idle_pool_tests.rs"]
mod tests;
