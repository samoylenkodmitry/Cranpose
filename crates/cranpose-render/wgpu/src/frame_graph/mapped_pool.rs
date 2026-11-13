use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

/// Buffers the CPU writes while they are mapped and the GPU reads once they
/// are unmapped.
pub(crate) trait MappedBuffers {
    fn mapped_buffers(&self) -> &[wgpu::Buffer];
}

struct MapProgress {
    remaining: AtomicUsize,
    failed: AtomicBool,
    // The count of the recall that asked for the maps, and the submit they
    // wait for, when the frame that unmapped the buffers submitted.
    recalled: OnceLock<(u64, wgpu::SubmissionIndex)>,
}

impl MapProgress {
    fn settled(&self) -> bool {
        self.failed.load(Ordering::Acquire) || self.remaining.load(Ordering::Acquire) == 0
    }
}

/// Map requests of buffers a frame unmapped, sent once the frame is
/// submitted: a map completes when the GPU is done with every submit that
/// read the buffer.
#[derive(Default)]
pub(crate) struct MapRequests {
    pending: Vec<(wgpu::Buffer, Arc<MapProgress>)>,
    recalls: u64,
}

impl MapRequests {
    // Asks for the maps of the buffers a frame unmapped, once `submission`
    // is the frame's submit; `None` when the caller does not know it.
    pub(crate) fn recall(&mut self, submission: Option<&wgpu::SubmissionIndex>) {
        self.recalls += 1;
        for (buffer, progress) in self.pending.drain(..) {
            if let Some(submission) = submission {
                let _ = progress.recalled.set((self.recalls, submission.clone()));
            }
            buffer.map_async(wgpu::MapMode::Write, .., move |result| match result {
                Ok(()) => {
                    progress.remaining.fetch_sub(1, Ordering::Release);
                }
                Err(_) => progress.failed.store(true, Ordering::Release),
            });
        }
    }
}

// The recycles in a row that find a free item no frame took: then the free
// item out of use longest goes, as the frames need one fewer.
const SPARE_RECYCLES: u8 = 8;

/// Items of mapped buffers out of use: free ones the CPU may write, and
/// ones waiting for their maps.
pub(crate) struct MappedPool<T> {
    free: Vec<T>,
    mapping: Vec<(T, Arc<MapProgress>)>,
    spare_recycles: u8,
}

impl<T> Default for MappedPool<T> {
    fn default() -> Self {
        Self {
            free: Vec::new(),
            mapping: Vec::new(),
            spare_recycles: 0,
        }
    }
}

impl<T: MappedBuffers> MappedPool<T> {
    /// Takes back `item`, unmapped and read by the frame about to be
    /// submitted, and asks for its buffers once `requests` is recalled.
    pub(crate) fn recycle(&mut self, item: T, requests: &mut MapRequests) {
        self.drop_spare();
        let buffers = item.mapped_buffers();
        let progress = Arc::new(MapProgress {
            remaining: AtomicUsize::new(buffers.len()),
            failed: AtomicBool::new(false),
            recalled: OnceLock::new(),
        });
        requests.pending.extend(
            buffers
                .iter()
                .map(|buffer| (buffer.clone(), Arc::clone(&progress))),
        );
        self.mapping.push((item, progress));
    }

    // The free item `choose` takes from the free ones, after a poll when
    // it takes none the first time and maps are pending. When it still
    // takes none and an item waits for a submit older than the newest, the
    // frame waits for that submit, which the GPU most likely finished: the
    // frames in flight reuse the items of the frames before them instead
    // of making more. Nothing waits for the newest submit, which the GPU
    // reads while the CPU records this frame.
    pub(crate) fn take(
        &mut self,
        device: &wgpu::Device,
        mut choose: impl FnMut(&mut Vec<T>) -> Option<T>,
    ) -> Option<T> {
        self.collect();
        if let Some(item) = choose(&mut self.free) {
            return Some(item);
        }
        if self.mapping.is_empty() {
            return None;
        }
        let _ = device.poll(wgpu::PollType::Poll);
        self.collect();
        if let Some(item) = choose(&mut self.free) {
            return Some(item);
        }
        let submission = self.older_submission()?;
        let _ = device.poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: None,
        });
        self.collect();
        choose(&mut self.free)
    }

    // Drops the free item out of use longest once recycles in a row found
    // one free: an item a frame grew into, or one the frames in flight
    // needed at their start, stays only while frames take it.
    fn drop_spare(&mut self) {
        if self.free.is_empty() {
            self.spare_recycles = 0;
            return;
        }
        self.spare_recycles += 1;
        if self.spare_recycles >= SPARE_RECYCLES {
            self.spare_recycles = 0;
            self.free.remove(0);
        }
    }

    // The submit the oldest item waits for, when a newer item waits for a
    // later recall.
    fn older_submission(&self) -> Option<wgpu::SubmissionIndex> {
        let (oldest, submission) = self.mapping.first()?.1.recalled.get()?;
        let (newest, _) = self.mapping.last()?.1.recalled.get()?;
        (oldest < newest).then(|| submission.clone())
    }

    // Moves the items whose maps completed to the free ones, and drops those
    // whose maps failed; the rest keep their order, oldest first.
    fn collect(&mut self) {
        for (item, progress) in self
            .mapping
            .extract_if(.., |(_, progress)| progress.settled())
        {
            if !progress.failed.load(Ordering::Acquire) {
                self.free.push(item);
            }
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.free.len() + self.mapping.len()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &T> {
        self.free
            .iter()
            .chain(self.mapping.iter().map(|(item, _)| item))
    }

    pub(crate) fn clear(&mut self) {
        self.free.clear();
        self.mapping.clear();
    }

    /// Drops the item waiting longest for its map.
    pub(crate) fn drop_oldest_mapping(&mut self) {
        if !self.mapping.is_empty() {
            self.mapping.remove(0);
        }
    }
}

/// Takes the largest of `free` that holds `fit` bytes, after dropping those
/// under half of it: they would not serve a frame like the last.
pub(crate) fn take_largest_fitting<T>(
    free: &mut Vec<T>,
    fit: u64,
    capacity: impl Fn(&T) -> u64,
) -> Option<T> {
    free.retain(|item| capacity(item) >= fit / 2);
    let largest = free
        .iter()
        .enumerate()
        .filter(|(_, item)| capacity(item) >= fit)
        .max_by_key(|(_, item)| capacity(item))
        .map(|(slot, _)| slot)?;
    Some(free.swap_remove(largest))
}
