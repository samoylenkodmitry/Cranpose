use std::{
    f32::consts::PI,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};

use cranpose_services::{EqualizerBand, OCTAVE_BAND_CENTERS_HZ};
use parking_lot::Mutex;

use crate::source::{ChannelCount, Sample, SampleRate, SampleSource, SeekError};

pub(crate) const BAND_CENTERS_HZ: [f32; 10] = OCTAVE_BAND_CENTERS_HZ;

pub(crate) const BAND_RANGE_DB: f32 = 12.0;

const BAND_Q: f32 = 1.41;

pub(crate) fn bands() -> Vec<EqualizerBand> {
    cranpose_services::octave_equalizer_bands(BAND_RANGE_DB)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Biquad {
    const PASSTHROUGH: Biquad = Biquad {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    fn peaking(center_hz: f32, sample_rate: f32, gain_db: f32) -> Biquad {
        if gain_db == 0.0 || sample_rate <= 0.0 || center_hz * 2.0 >= sample_rate {
            return Biquad::PASSTHROUGH;
        }
        let amplitude = 10f32.powf(gain_db / 40.0);
        let omega = 2.0 * PI * center_hz / sample_rate;
        let (sin_omega, cos_omega) = omega.sin_cos();
        let alpha = sin_omega / (2.0 * BAND_Q);
        let a0 = 1.0 + alpha / amplitude;
        Biquad {
            b0: (1.0 + alpha * amplitude) / a0,
            b1: (-2.0 * cos_omega) / a0,
            b2: (1.0 - alpha * amplitude) / a0,
            a1: (-2.0 * cos_omega) / a0,
            a2: (1.0 - alpha / amplitude) / a0,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct BiquadState {
    z1: f32,
    z2: f32,
}

impl BiquadState {
    #[inline]
    fn process(&mut self, filter: &Biquad, input: f32) -> f32 {
        let output = filter.b0 * input + self.z1;
        self.z1 = filter.b1 * input - filter.a1 * output + self.z2;
        self.z2 = filter.b2 * input - filter.a2 * output;
        output
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Curve {
    preamp: f32,
    filters: Vec<Biquad>,
}

impl Curve {
    fn flat() -> Curve {
        Curve {
            preamp: 1.0,
            filters: Vec::new(),
        }
    }

    fn build(sample_rate: f32, preamp_db: f32, gains_db: &[f32]) -> Curve {
        Curve {
            preamp: 10f32.powf(preamp_db / 20.0),
            filters: BAND_CENTERS_HZ
                .iter()
                .enumerate()
                .map(|(index, center)| {
                    let gain = gains_db.get(index).copied().unwrap_or(0.0);
                    Biquad::peaking(*center, sample_rate, gain)
                })
                .collect(),
        }
    }
}

pub(crate) struct EqualizerTap {
    enabled: AtomicBool,
    setting: Mutex<(f32, Vec<f32>)>,
    sample_rate: AtomicU32,
    curve: Mutex<Arc<Curve>>,
}

impl EqualizerTap {
    pub(crate) fn new() -> Arc<EqualizerTap> {
        Arc::new(EqualizerTap {
            enabled: AtomicBool::new(false),
            setting: Mutex::new((0.0, Vec::new())),
            sample_rate: AtomicU32::new(0),
            curve: Mutex::new(Arc::new(Curve::flat())),
        })
    }

    pub(crate) fn set(&self, enabled: bool, preamp_db: f32, gains_db: &[f32]) {
        *self.setting.lock() = (preamp_db, gains_db.to_vec());
        self.enabled.store(enabled, Ordering::Release);
        self.rebuild();
    }

    pub(crate) fn prepare(&self, sample_rate: u32) {
        *self.curve.lock() = Arc::new(Curve::flat());
        self.sample_rate.store(sample_rate, Ordering::Release);
        self.rebuild();
    }

    fn rebuild(&self) {
        let rate = self.sample_rate.load(Ordering::Acquire);
        if rate == 0 {
            return;
        }
        let (preamp_db, gains) = self.setting.lock().clone();
        *self.curve.lock() = Arc::new(Curve::build(rate as f32, preamp_db, &gains));
    }

    fn published_curve(&self) -> Option<Option<Arc<Curve>>> {
        if !self.enabled.load(Ordering::Acquire) {
            return Some(None);
        }
        self.curve.try_lock().map(|curve| Some(Arc::clone(&curve)))
    }

    fn current_curve(&self) -> Option<Arc<Curve>> {
        self.enabled
            .load(Ordering::Acquire)
            .then(|| Arc::clone(&self.curve.lock()))
    }

    pub(crate) fn wrap<S: SampleSource>(self: &Arc<Self>, inner: S) -> EqualizerSource<S> {
        let channels = inner.channels().get() as usize;
        self.prepare(inner.sample_rate().get());
        EqualizerSource {
            states: vec![[BiquadState::default(); BAND_CENTERS_HZ.len()]; channels],
            channels,
            channel: 0,
            curve: self.current_curve(),
            refresh: 0,
            tap: Arc::clone(self),
            inner,
        }
    }
}

const CURVE_REFRESH_SAMPLES: u32 = 512;

pub(crate) struct EqualizerSource<S> {
    inner: S,
    tap: Arc<EqualizerTap>,
    states: Vec<[BiquadState; BAND_CENTERS_HZ.len()]>,
    channels: usize,
    channel: usize,
    curve: Option<Arc<Curve>>,
    refresh: u32,
}

impl<S> EqualizerSource<S> {
    fn reset(&mut self) {
        for channel in &mut self.states {
            *channel = [BiquadState::default(); BAND_CENTERS_HZ.len()];
        }
        self.channel = 0;
    }
}

impl<S: SampleSource> Iterator for EqualizerSource<S> {
    type Item = Sample;

    #[inline]
    fn next(&mut self) -> Option<Sample> {
        let sample = self.inner.next()?;

        self.refresh = self.refresh.saturating_add(1);
        if self.refresh >= CURVE_REFRESH_SAMPLES {
            self.refresh = 0;
            if let Some(next) = self.tap.published_curve() {
                if next.is_none() != self.curve.is_none() {
                    self.reset();
                }
                self.curve = next;
            }
        }

        let Some(curve) = &self.curve else {
            return Some(sample);
        };
        let channel = self.channel;
        self.channel = if channel + 1 >= self.channels {
            0
        } else {
            channel + 1
        };
        let Some(states) = self.states.get_mut(channel) else {
            return Some(sample);
        };

        let mut value = sample * curve.preamp;
        for (state, filter) in states.iter_mut().zip(curve.filters.iter()) {
            value = state.process(filter, value);
        }
        Some(value.clamp(-1.0, 1.0))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S: SampleSource> SampleSource for EqualizerSource<S> {
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
        self.reset();
        self.inner.try_seek(position)
    }
}

#[cfg(test)]
#[path = "tests/equalizer_tests.rs"]
mod tests;
