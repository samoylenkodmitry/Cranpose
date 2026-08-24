//! What a decoded item looks like on its way to the device.
//!
//! One interleaved stream of `f32` samples, plus the two facts every stage
//! needs to interpret it — how many channels a frame has and how many frames a
//! second holds — and the two an item has that a plain iterator does not: how
//! long it is and where to move to. The equalizer and the analysis tap are
//! adapters over this trait, so the chain that reaches the device is
//! `decoder -> equalizer -> analysis` with nothing else in it.

use std::{
    num::{NonZeroU16, NonZeroU32},
    time::Duration,
};

/// One interleaved sample.
pub(crate) type Sample = f32;

/// Channels per frame.
pub(crate) type ChannelCount = NonZeroU16;

/// Frames per second.
pub(crate) type SampleRate = NonZeroU32;

/// Why a seek did not happen.
#[derive(Debug)]
pub(crate) enum SeekError {
    /// The container cannot seek — a live stream, or a format with no index and
    /// no constant bitrate to estimate from.
    Unsupported,
    /// The container tried and failed.
    Failed(String),
}

impl std::fmt::Display for SeekError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SeekError::Unsupported => write!(formatter, "this item cannot seek"),
            SeekError::Failed(reason) => write!(formatter, "{reason}"),
        }
    }
}

impl std::error::Error for SeekError {}

/// A stream of interleaved samples that knows its own shape.
///
/// `Send` because the decode thread owns the chain and the sink hands it over
/// when a session starts.
pub(crate) trait SampleSource: Iterator<Item = Sample> + Send {
    /// Channels per frame. Samples arrive interleaved in this many.
    fn channels(&self) -> ChannelCount;

    /// Frames per second of the samples this source yields.
    fn sample_rate(&self) -> SampleRate;

    /// How long the item runs, when the container says.
    fn total_duration(&self) -> Option<Duration>;

    /// Moves the read position. On `Err` the position is unchanged.
    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError>;
}

/// Samples already in memory, for tests.
#[cfg(test)]
pub(crate) struct SamplesBuffer {
    samples: std::vec::IntoIter<Sample>,
    channels: ChannelCount,
    sample_rate: SampleRate,
    duration: Duration,
}

#[cfg(test)]
impl SamplesBuffer {
    /// Wraps `samples`, which are interleaved in `channels`.
    pub(crate) fn new(channels: u16, sample_rate: u32, samples: Vec<Sample>) -> SamplesBuffer {
        let channels = ChannelCount::new(channels).expect("channels");
        let sample_rate = SampleRate::new(sample_rate).expect("sample rate");
        let frames = samples.len() / usize::from(channels.get());
        SamplesBuffer {
            duration: Duration::from_secs_f64(f64::from(sample_rate.get()).recip() * frames as f64),
            samples: samples.into_iter(),
            channels,
            sample_rate,
        }
    }
}

#[cfg(test)]
impl Iterator for SamplesBuffer {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        self.samples.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.samples.size_hint()
    }
}

#[cfg(test)]
impl SampleSource for SamplesBuffer {
    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        Some(self.duration)
    }

    fn try_seek(&mut self, _position: Duration) -> Result<(), SeekError> {
        Err(SeekError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_buffer_reports_the_shape_it_was_built_with() {
        let buffer = SamplesBuffer::new(2, 48_000, vec![0.0; 96_000]);
        assert_eq!(buffer.channels().get(), 2);
        assert_eq!(buffer.sample_rate().get(), 48_000);
        assert_eq!(buffer.total_duration(), Some(Duration::from_secs(1)));
    }

    #[test]
    fn a_buffer_yields_every_sample_once() {
        let buffer = SamplesBuffer::new(1, 8_000, vec![0.25, -0.5, 1.0]);
        assert_eq!(buffer.collect::<Vec<_>>(), vec![0.25, -0.5, 1.0]);
    }

    #[test]
    fn a_seek_error_says_which_kind_it_is() {
        assert_eq!(SeekError::Unsupported.to_string(), "this item cannot seek");
        assert_eq!(
            SeekError::Failed("no index".to_owned()).to_string(),
            "no index"
        );
    }
}
