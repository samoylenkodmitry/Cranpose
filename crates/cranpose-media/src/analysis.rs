//! The sample tap a visualiser reads.
//!
//! The samples a visualiser wants are the ones on their way to the device, so
//! the tap sits in the output callback — which is a real-time thread. Nothing
//! here allocates, blocks or logs on that side: a full block is copied into a
//! slot the progress thread drains, and a block that arrives while the slot is
//! held is counted and dropped rather than made to wait.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering},
    },
    time::Duration,
};

use cranpose_services::{MediaSamples, publish_media_samples, record_dropped_media_samples};
use parking_lot::Mutex;

use crate::source::{ChannelCount, Sample, SampleRate, SampleSource, SeekError};

/// How many frames make up one published block.
///
/// 1024 frames is ~23 ms at 44.1 kHz: short enough that a visualiser drawn at
/// 60 Hz has a fresh block most frames, long enough to be a useful window for
/// a spectrum.
pub(crate) const ANALYSIS_FRAMES: usize = 1024;

/// The slot the output callback fills and the progress thread drains.
pub(crate) struct AnalysisTap {
    enabled: AtomicBool,
    filled: AtomicBool,
    ready: Mutex<Vec<f32>>,
    sequence: AtomicU64,
    sample_rate: AtomicU32,
    channels: AtomicU16,
}

impl AnalysisTap {
    pub(crate) fn new() -> Arc<AnalysisTap> {
        Arc::new(AnalysisTap {
            enabled: AtomicBool::new(false),
            filled: AtomicBool::new(false),
            ready: Mutex::new(Vec::new()),
            sequence: AtomicU64::new(0),
            sample_rate: AtomicU32::new(0),
            channels: AtomicU16::new(0),
        })
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
        if !enabled {
            self.filled.store(false, Ordering::Release);
        }
    }

    /// Sizes the slot for the item about to play. Called from the UI thread,
    /// before the source reaches the device, so the output callback never has
    /// to grow anything.
    fn prepare(&self, sample_rate: u32, channels: u16) {
        self.sample_rate.store(sample_rate, Ordering::Release);
        self.channels.store(channels, Ordering::Release);
        self.filled.store(false, Ordering::Release);
        let mut ready = self.ready.lock();
        ready.clear();
        ready.resize(ANALYSIS_FRAMES * channels.max(1) as usize, 0.0);
    }

    /// Takes a full block from the output callback.
    ///
    /// `try_lock` rather than `lock`: the drain side holds the slot for the
    /// length of one copy, and waiting for it here would be waiting inside the
    /// device callback.
    fn offer(&self, block: &[f32]) {
        let Some(mut ready) = self.ready.try_lock() else {
            record_dropped_media_samples();
            return;
        };
        if ready.len() != block.len() {
            record_dropped_media_samples();
            return;
        }
        ready.copy_from_slice(block);
        self.sequence.fetch_add(1, Ordering::AcqRel);
        self.filled.store(true, Ordering::Release);
    }

    /// Publishes whatever the output callback left, if anything. Called from
    /// the progress thread.
    pub(crate) fn drain(&self) {
        if !self.filled.swap(false, Ordering::AcqRel) {
            return;
        }
        let block = self.ready.lock().clone();
        let samples = MediaSamples::new(
            self.sample_rate.load(Ordering::Acquire),
            self.channels.load(Ordering::Acquire),
            self.sequence.load(Ordering::Acquire),
            block,
        );
        if let Some(samples) = samples {
            publish_media_samples(samples);
        }
    }

    /// Wraps `inner` so its samples pass through this tap on their way to the
    /// device.
    pub(crate) fn wrap<S: SampleSource>(self: &Arc<Self>, inner: S) -> AnalysisSource<S> {
        let sample_rate = inner.sample_rate();
        let channels = inner.channels();
        self.prepare(sample_rate.get(), channels.get());
        AnalysisSource {
            staging: Vec::with_capacity(ANALYSIS_FRAMES * channels.get() as usize),
            capacity: ANALYSIS_FRAMES * channels.get() as usize,
            tap: Arc::clone(self),
            inner,
        }
    }
}

/// A source that copies what passes through it into an [`AnalysisTap`].
pub(crate) struct AnalysisSource<S> {
    inner: S,
    tap: Arc<AnalysisTap>,
    staging: Vec<f32>,
    capacity: usize,
}

impl<S: SampleSource> Iterator for AnalysisSource<S> {
    type Item = Sample;

    #[inline]
    fn next(&mut self) -> Option<Sample> {
        let sample = self.inner.next()?;
        if !self.tap.is_enabled() {
            if !self.staging.is_empty() {
                self.staging.clear();
            }
            return Some(sample);
        }
        self.staging.push(sample);
        if self.staging.len() >= self.capacity {
            self.tap.offer(&self.staging);
            self.staging.clear();
        }
        Some(sample)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S: SampleSource> SampleSource for AnalysisSource<S> {
    #[inline]
    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    #[inline]
    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    #[inline]
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
        self.staging.clear();
        self.inner.try_seek(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SamplesBuffer;

    fn tone(frames: usize, channels: u16) -> SamplesBuffer {
        SamplesBuffer::new(channels, 48_000, vec![0.25f32; frames * channels as usize])
    }

    #[test]
    fn a_disabled_tap_lets_the_samples_through_untouched() {
        let tap = AnalysisTap::new();
        let mut source = tap.wrap(tone(ANALYSIS_FRAMES * 2, 2));

        let played: Vec<f32> = source.by_ref().collect();

        assert_eq!(played.len(), ANALYSIS_FRAMES * 4);
        assert!(played.iter().all(|sample| *sample == 0.25));
        assert!(!tap.filled.load(Ordering::Acquire));
    }

    #[test]
    fn an_enabled_tap_publishes_one_block_at_a_time() {
        let tap = AnalysisTap::new();
        let mut source = tap.wrap(tone(ANALYSIS_FRAMES * 2, 2));
        tap.set_enabled(true);

        // One frame short of a block: nothing to drain yet.
        for _ in 0..ANALYSIS_FRAMES * 2 - 2 {
            source.next().expect("samples");
        }
        assert!(!tap.filled.load(Ordering::Acquire));

        source.next().expect("samples");
        source.next().expect("samples");
        assert!(tap.filled.load(Ordering::Acquire));
        assert_eq!(tap.ready.lock().len(), ANALYSIS_FRAMES * 2);
        assert_eq!(tap.sequence.load(Ordering::Acquire), 1);
    }

    #[test]
    fn a_block_that_nobody_took_is_replaced_rather_than_queued() {
        let tap = AnalysisTap::new();
        let mut source = tap.wrap(tone(ANALYSIS_FRAMES * 4, 1));
        tap.set_enabled(true);

        for _ in 0..ANALYSIS_FRAMES * 3 {
            source.next().expect("samples");
        }

        assert_eq!(tap.sequence.load(Ordering::Acquire), 3);
        assert!(tap.filled.load(Ordering::Acquire));
    }

    #[test]
    fn the_output_callback_never_grows_its_staging_buffer() {
        let tap = AnalysisTap::new();
        let mut source = tap.wrap(tone(ANALYSIS_FRAMES * 2, 2));
        tap.set_enabled(true);
        let capacity = source.staging.capacity();

        for _ in 0..ANALYSIS_FRAMES * 4 {
            source.next().expect("samples");
        }

        assert_eq!(source.staging.capacity(), capacity);
    }

    #[test]
    fn turning_the_tap_off_mid_item_stops_it_collecting() {
        let tap = AnalysisTap::new();
        let mut source = tap.wrap(tone(ANALYSIS_FRAMES * 2, 1));
        tap.set_enabled(true);
        for _ in 0..8 {
            source.next().expect("samples");
        }
        assert_eq!(source.staging.len(), 8);

        tap.set_enabled(false);
        source.next().expect("samples");

        assert!(source.staging.is_empty());
    }

    #[test]
    fn a_seek_throws_away_the_part_block_from_before_it() {
        let tap = AnalysisTap::new();
        let mut source = tap.wrap(tone(ANALYSIS_FRAMES * 2, 1));
        tap.set_enabled(true);
        for _ in 0..8 {
            source.next().expect("samples");
        }

        let _ = source.try_seek(Duration::ZERO);

        assert!(source.staging.is_empty());
    }
}
