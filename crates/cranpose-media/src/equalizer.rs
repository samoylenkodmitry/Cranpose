//! The equalizer the desktop backend puts in front of the output device.
//!
//! Ten peaking biquads per channel on the standard octave centres, plus a
//! preamp. The filters run in the source chain, which is a real-time path:
//! nothing here allocates or locks per sample. A setting change is published
//! into a shared cell and picked up by the next sample, so applying a curve
//! never blocks the audio thread or interrupts what is playing.

use std::{
    f32::consts::PI,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use cranpose_services::{EqualizerBand, OCTAVE_BAND_CENTERS_HZ};
use parking_lot::Mutex;

use crate::source::{ChannelCount, Sample, SampleRate, SampleSource, SeekError};

/// The centres this backend's filters sit on. The contract's own set, so a
/// curve saved on a desktop means the same thing in a browser.
pub(crate) const BAND_CENTERS_HZ: [f32; 10] = OCTAVE_BAND_CENTERS_HZ;

/// How far a band can lift or cut, in decibels.
pub(crate) const BAND_RANGE_DB: f32 = 12.0;

/// The Q of each peaking filter. One octave between centres works out at
/// roughly this, which is what makes ten bands cover the spectrum evenly
/// rather than leaving dips between them.
const BAND_Q: f32 = 1.41;

/// The bands this backend reports.
pub(crate) fn bands() -> Vec<EqualizerBand> {
    cranpose_services::octave_equalizer_bands(BAND_RANGE_DB)
}

/// A peaking filter's coefficients, in the transposed direct form 2 the
/// [`BiquadState`] below evaluates.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Biquad {
    /// The identity filter: what a band set to 0 dB is.
    const PASSTHROUGH: Biquad = Biquad {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    /// A peaking EQ section, from the Audio EQ Cookbook.
    ///
    /// A centre at or above the Nyquist frequency has no filter to build — the
    /// 16 kHz band on a 22 kHz recording, say — and passes through instead of
    /// producing coefficients that would ring.
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

/// One filter's memory for one channel.
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

/// A curve, resolved for one sample rate.
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

/// The curve the source reads, and the switch that takes it out of circuit.
///
/// The curve is rebuilt whenever the setting or the sample rate changes, which
/// happens on the UI thread; the audio thread only ever reads it.
pub(crate) struct EqualizerTap {
    enabled: AtomicBool,
    /// The setting as given, kept so a new item at a different sample rate can
    /// rebuild the same curve for its own rate.
    setting: Mutex<(f32, Vec<f32>)>,
    /// The rate the coefficients were built for. A new item can open at a
    /// different one, and the same curve then means different filters.
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

    /// Applies a setting. Takes effect on the next sample of whatever is
    /// already playing.
    pub(crate) fn set(&self, enabled: bool, preamp_db: f32, gains_db: &[f32]) {
        *self.setting.lock() = (preamp_db, gains_db.to_vec());
        self.enabled.store(enabled, Ordering::Release);
        self.rebuild();
    }

    /// Rebuilds the curve for `sample_rate`. Called when an item opens, since
    /// the coefficients depend on the rate the samples arrive at.
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

    /// The curve to run now, or `None` when the equalizer is out of circuit.
    ///
    /// The outer `None` means the cell was being written and this call must
    /// keep whatever it already had. `try_lock` rather than `lock` because the
    /// caller is the audio thread: a curve that arrives one refresh interval
    /// later is inaudible, and a blocked output callback is not.
    fn published_curve(&self) -> Option<Option<Arc<Curve>>> {
        if !self.enabled.load(Ordering::Acquire) {
            return Some(None);
        }
        self.curve.try_lock().map(|curve| Some(Arc::clone(&curve)))
    }

    /// The same, for the thread that opens an item and can afford to wait.
    fn current_curve(&self) -> Option<Arc<Curve>> {
        self.enabled
            .load(Ordering::Acquire)
            .then(|| Arc::clone(&self.curve.lock()))
    }

    /// Puts the equalizer in front of `inner`.
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

/// How many samples pass between checks for a new curve. Reading the shared
/// cell takes a lock, and a curve that arrives 512 samples (~11 ms) late is not
/// something anyone hears; taking that lock per sample on the audio thread is.
const CURVE_REFRESH_SAMPLES: u32 = 512;

/// A source that runs the equalizer over what passes through it.
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
                // Switching the equalizer in or out starts filters whose memory
                // is from a signal that was not filtered the same way, which is
                // a click. Clearing it costs one block of ramp instead.
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
        // Filter memory is the tail of the samples that just played. Carrying
        // it across a seek rings the first block of the new position with the
        // old one, which is audible as a click.
        self.reset();
        self.inner.try_seek(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SamplesBuffer;

    /// A sine at `hz`, one channel, one second.
    fn tone(hz: f32, sample_rate: u32) -> SamplesBuffer {
        let frames = sample_rate as usize;
        let samples: Vec<f32> = (0..frames)
            .map(|frame| (2.0 * PI * hz * frame as f32 / sample_rate as f32).sin())
            .collect();
        SamplesBuffer::new(1, sample_rate, samples)
    }

    /// Peak amplitude of the second half, so the filters have settled.
    fn settled_peak(samples: &[f32]) -> f32 {
        samples[samples.len() / 2..]
            .iter()
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
    }

    fn run(tap: &Arc<EqualizerTap>, hz: f32, sample_rate: u32) -> Vec<f32> {
        tap.wrap(tone(hz, sample_rate)).collect()
    }

    #[test]
    fn a_disabled_equalizer_passes_every_sample_through_unchanged() {
        let tap = EqualizerTap::new();
        tap.set(false, 0.0, &[12.0; BAND_CENTERS_HZ.len()]);
        let source: Vec<f32> = tone(1_000.0, 44_100).collect();
        let filtered = run(&tap, 1_000.0, 44_100);
        assert_eq!(source, filtered, "a disabled equalizer must be a no-op");
    }

    #[test]
    fn an_enabled_flat_equalizer_leaves_the_level_where_it_was() {
        let tap = EqualizerTap::new();
        tap.set(true, 0.0, &[0.0; BAND_CENTERS_HZ.len()]);
        let filtered = run(&tap, 1_000.0, 44_100);
        let peak = settled_peak(&filtered);
        assert!(
            (peak - 1.0).abs() < 0.02,
            "flat bands changed the level: peak {peak}"
        );
    }

    #[test]
    fn lifting_a_band_lifts_the_frequency_it_is_centred_on() {
        let mut gains = [0.0f32; BAND_CENTERS_HZ.len()];
        // The 1 kHz band.
        gains[5] = 12.0;
        let tap = EqualizerTap::new();
        tap.set(true, 0.0, &gains);

        let lifted = settled_peak(&run(&tap, 1_000.0, 44_100));
        // +12 dB is a factor of ~4, and the source peaks at 1.0, so the output
        // is clamped. What must be true is that it reached the ceiling.
        assert!(lifted > 0.98, "the centred band was not lifted: {lifted}");

        // Three octaves down is outside this band's reach.
        let untouched = settled_peak(&run(&tap, 125.0, 44_100));
        assert!(
            untouched < 1.15,
            "a band three octaves away moved: {untouched}"
        );
    }

    #[test]
    fn cutting_a_band_cuts_the_frequency_it_is_centred_on() {
        let mut gains = [0.0f32; BAND_CENTERS_HZ.len()];
        gains[5] = -12.0;
        let tap = EqualizerTap::new();
        tap.set(true, 0.0, &gains);

        let cut = settled_peak(&run(&tap, 1_000.0, 44_100));
        assert!(cut < 0.4, "the centred band was not cut: {cut}");
    }

    #[test]
    fn the_preamp_scales_everything_ahead_of_the_bands() {
        let tap = EqualizerTap::new();
        tap.set(true, -6.0, &[0.0; BAND_CENTERS_HZ.len()]);
        let peak = settled_peak(&run(&tap, 1_000.0, 44_100));
        // -6 dB is a factor of ~0.5.
        assert!(
            (peak - 0.5).abs() < 0.03,
            "the preamp did not halve the level: {peak}"
        );
    }

    #[test]
    fn a_band_above_the_nyquist_frequency_is_left_alone() {
        // 16 kHz on a 22.05 kHz recording is past what the samples can carry.
        // Building coefficients for it produces a filter that rings.
        let filter = Biquad::peaking(16_000.0, 22_050.0, 12.0);
        assert_eq!(
            filter,
            Biquad::PASSTHROUGH,
            "a band at or above Nyquist must pass through"
        );
    }

    #[test]
    fn every_reported_band_can_be_lifted_and_cut_by_the_same_amount() {
        let bands = bands();
        assert_eq!(bands.len(), BAND_CENTERS_HZ.len());
        for (band, center) in bands.iter().zip(BAND_CENTERS_HZ.iter()) {
            assert_eq!(band.center_hz, *center);
            assert_eq!(band.max_gain_db, BAND_RANGE_DB);
            assert_eq!(band.min_gain_db, -BAND_RANGE_DB);
            assert_eq!(band.clamp_gain(99.0), BAND_RANGE_DB);
            assert_eq!(band.clamp_gain(-99.0), -BAND_RANGE_DB);
        }
    }

    #[test]
    fn a_new_item_rebuilds_the_curve_for_its_own_sample_rate() {
        let tap = EqualizerTap::new();
        let mut gains = [0.0f32; BAND_CENTERS_HZ.len()];
        gains[9] = 12.0;
        tap.set(true, 0.0, &gains);

        // 16 kHz is a real band at 44.1 kHz and past Nyquist at 22.05 kHz, so
        // the same setting must produce different filters for the two rates.
        tap.prepare(44_100);
        let wide = tap
            .current_curve()
            .expect("an enabled equalizer has a curve");
        tap.prepare(22_050);
        let narrow = tap
            .current_curve()
            .expect("an enabled equalizer has a curve");

        assert_ne!(wide.filters[9], Biquad::PASSTHROUGH);
        assert_eq!(narrow.filters[9], Biquad::PASSTHROUGH);
    }
}
