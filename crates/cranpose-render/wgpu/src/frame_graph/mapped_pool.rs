use std::sync::{
    Arc,
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
}

/// Map requests of buffers a frame unmapped, sent once the frame is
/// submitted: a map completes when the GPU is done with every submit that
/// read the buffer.
#[derive(Default)]
pub(crate) struct MapRequests(Vec<(wgpu::Buffer, Arc<MapProgress>)>);

impl MapRequests {
    pub(crate) fn recall(&mut self) {
        for (buffer, progress) in self.0.drain(..) {
            buffer.map_async(wgpu::MapMode::Write, .., move |result| match result {
                Ok(()) => {
                    progress.remaining.fetch_sub(1, Ordering::Release);
                }
                Err(_) => progress.failed.store(true, Ordering::Release),
            });
        }
    }
}

/// Items of mapped buffers out of use: free ones the CPU may write, and
/// ones waiting for their maps.
pub(crate) struct MappedPool<T> {
    free: Vec<T>,
    mapping: Vec<(T, Arc<MapProgress>)>,
}

impl<T> Default for MappedPool<T> {
    fn default() -> Self {
        Self {
            free: Vec::new(),
            mapping: Vec::new(),
        }
    }
}

impl<T: MappedBuffers> MappedPool<T> {
    /// Takes back `item`, unmapped and read by the frame about to be
    /// submitted, and asks for its buffers once `requests` is recalled.
    pub(crate) fn recycle(&mut self, item: T, requests: &mut MapRequests) {
        let buffers = item.mapped_buffers();
        let progress = Arc::new(MapProgress {
            remaining: AtomicUsize::new(buffers.len()),
            failed: AtomicBool::new(false),
        });
        requests.0.extend(
            buffers
                .iter()
                .map(|buffer| (buffer.clone(), Arc::clone(&progress))),
        );
        self.mapping.push((item, progress));
    }

    /// The free item `choose` takes from the free ones, after a poll when
    /// it takes none the first time and maps are pending.
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
        choose(&mut self.free)
    }

    fn collect(&mut self) {
        let mut index = 0;
        while index < self.mapping.len() {
            let progress = &self.mapping[index].1;
            if progress.failed.load(Ordering::Acquire) {
                self.mapping.swap_remove(index);
            } else if progress.remaining.load(Ordering::Acquire) == 0 {
                let (item, _) = self.mapping.swap_remove(index);
                self.free.push(item);
            } else {
                index += 1;
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
