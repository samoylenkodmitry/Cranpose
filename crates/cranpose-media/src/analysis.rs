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

pub(crate) const ANALYSIS_FRAMES: usize = 1024;

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

    fn prepare(&self, sample_rate: u32, channels: u16) {
        self.sample_rate.store(sample_rate, Ordering::Release);
        self.channels.store(channels, Ordering::Release);
        self.filled.store(false, Ordering::Release);
        let mut ready = self.ready.lock();
        ready.clear();
        ready.resize(ANALYSIS_FRAMES * channels.max(1) as usize, 0.0);
    }

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
#[path = "tests/analysis_tests.rs"]
mod tests;
