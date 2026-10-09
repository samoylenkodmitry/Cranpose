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

    /// Ends a frame like [`Self::end_frame`], after dropping the oldest of
    /// the items handed back before this frame while more than `max_idle` of
    /// them are held. The items this frame handed back all wait for the
    /// next frame.
    pub(crate) fn end_frame_keeping(&mut self, max_idle: usize) {
        let earlier = self
            .available
            .partition_point(|(_, returned)| *returned < self.frame);
        self.available.drain(..earlier.saturating_sub(max_idle));
        self.end_frame();
    }

    /// Ends a frame, dropping every item no frame has taken back out for
    /// [`IDLE_FRAMES`].
    pub(crate) fn end_frame(&mut self) {
        self.frame += 1;
        let oldest_kept = self.frame.saturating_sub(IDLE_FRAMES);
        self.available
            .retain(|(_, returned)| *returned >= oldest_kept);
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &T> {
        self.available.iter().map(|(item, _)| item)
    }

    pub(crate) fn len(&self) -> usize {
        self.available.len()
    }
}

/// The most a frame of the last [`IDLE_FRAMES`] asked for. A busier frame
/// raises it at once; once the peak is that old, it follows the frames that
/// came since.
#[derive(Default)]
pub(crate) struct RecentPeak {
    frame: u64,
    peak: u64,
    frames_since_peak: u64,
}

impl RecentPeak {
    /// Adds `amount` to what this frame asks for.
    pub(crate) fn add(&mut self, amount: u64) {
        self.frame = self.frame.saturating_add(amount);
    }

    /// Raises what this frame asks for to at least `amount`.
    pub(crate) fn reach(&mut self, amount: u64) {
        self.frame = self.frame.max(amount);
    }

    /// The peak, or this frame's demand when that is higher.
    pub(crate) fn value(&self) -> u64 {
        self.peak.max(self.frame)
    }

    pub(crate) fn end_frame(&mut self) {
        self.frames_since_peak = self.frames_since_peak.saturating_add(1);
        if self.frame >= self.peak || self.frames_since_peak > IDLE_FRAMES {
            self.peak = self.frame;
            self.frames_since_peak = 0;
        }
        self.frame = 0;
    }
}

/// A vector a renderer fills again every frame, keeping its capacity between
/// frames so it never reallocates. Once the frames that need the most have
/// passed [`IDLE_FRAMES`] ago, it gives back what is over twice the recent
/// peak: a burst at startup or a scene the app has left holds no memory,
/// and a frame that needs a little more than the peak doubles back within
/// the bound, so nothing shrinks and grows from frame to frame.
pub(crate) struct FrameScratch<T> {
    items: Vec<T>,
    peak: RecentPeak,
}

impl<T> Default for FrameScratch<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            peak: RecentPeak::default(),
        }
    }
}

impl<T> FrameScratch<T> {
    /// Empties the vector for its next use, noting the length the last use
    /// reached.
    pub(crate) fn clear(&mut self) {
        self.peak.reach(self.items.len() as u64);
        self.items.clear();
    }

    /// Ends a frame, emptying the vector after its last use.
    pub(crate) fn end_frame(&mut self) {
        self.clear();
        self.peak.end_frame();
        let peak = usize::try_from(self.peak.value()).unwrap_or(usize::MAX);
        if self.items.capacity() > peak.saturating_mul(2) {
            self.items.shrink_to(peak);
        }
    }
}

impl<T> std::ops::Deref for FrameScratch<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Vec<T> {
        &self.items
    }
}

impl<T> std::ops::DerefMut for FrameScratch<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.items
    }
}

#[cfg(test)]
#[path = "tests/idle_pool_tests.rs"]
mod tests;
