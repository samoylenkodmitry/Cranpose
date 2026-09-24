use std::{
    num::{NonZeroU16, NonZeroU32},
    sync::Arc,
    time::Duration,
};

pub(crate) type Sample = f32;

pub(crate) type ChannelCount = NonZeroU16;

pub(crate) type SampleRate = NonZeroU32;

#[derive(Debug)]
pub(crate) enum SeekError {
    Unsupported,
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

#[derive(Clone, Default)]
pub(crate) struct SourceCancel {
    stop: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl SourceCancel {
    pub(crate) fn new(stop: impl Fn() + Send + Sync + 'static) -> SourceCancel {
        SourceCancel {
            stop: Some(Arc::new(stop)),
        }
    }

    pub(crate) fn cancel(&self) {
        if let Some(stop) = self.stop.as_ref() {
            stop();
        }
    }
}

pub(crate) trait SampleSource: Iterator<Item = Sample> + Send {
    fn channels(&self) -> ChannelCount;

    fn sample_rate(&self) -> SampleRate;

    fn total_duration(&self) -> Option<Duration>;

    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError>;
}

#[cfg(test)]
pub(crate) struct SamplesBuffer {
    samples: std::vec::IntoIter<Sample>,
    channels: ChannelCount,
    sample_rate: SampleRate,
    duration: Duration,
}

#[cfg(test)]
impl SamplesBuffer {
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
#[path = "tests/source_tests.rs"]
mod tests;
