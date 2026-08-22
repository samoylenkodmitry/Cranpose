//! Decoding an item into the sample stream the sink plays.
//!
//! `symphonia` reads the container and the codec; this turns what it yields —
//! a packet at a time, in whatever layout the codec produced — into one
//! interleaved `f32` stream that satisfies [`SampleSource`].
//!
//! A packet decodes into many frames, so the decoded block is held and drained
//! sample by sample. That is what lets the equalizer and the analysis tap stay
//! plain iterator adapters instead of each having to understand packets.

use crate::source::{ChannelCount, Sample, SampleRate, SampleSource, SeekError};
use cranpose_services::MediaError;
use std::fs::File;
use std::path::Path;
use std::time::Duration;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Duration as TimeBaseUnits, Time, TimeBase};

/// One item, open and decoding.
pub(crate) struct Decoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    channels: ChannelCount,
    sample_rate: SampleRate,
    total_duration: Option<Duration>,
    /// The last decoded packet, interleaved, and how far into it we have read.
    block: Vec<Sample>,
    read: usize,
    /// Set once the container has no more packets, so a drained block ends the
    /// stream instead of asking for another one.
    exhausted: bool,
    /// Frames still to be thrown away after a seek.
    ///
    /// A container seeks to a packet, not to a frame, so it lands at or before
    /// what was asked for and reports both timestamps. Dropping the difference
    /// is what turns a packet-accurate seek into a frame-accurate one; without
    /// it a seek silently plays from up to a packet early.
    skip_frames: u64,
}

impl Decoder {
    /// Opens `path` and prepares its first audio track.
    ///
    /// Nothing is decoded here beyond what the probe needs, so opening an item
    /// costs the container's header rather than its audio.
    pub(crate) fn open(path: &Path) -> Result<Decoder, MediaError> {
        let file = File::open(path)
            .map_err(|error| MediaError::Failed(format!("{}: {error}", path.display())))?;
        let stream = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
            hint.with_extension(extension);
        }

        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                stream,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|error| MediaError::Failed(format!("{}: {error}", path.display())))?;

        Decoder::from_format(format)
    }

    fn from_format(format: Box<dyn FormatReader>) -> Result<Decoder, MediaError> {
        let track = format
            .first_track(TrackType::Audio)
            .ok_or_else(|| MediaError::Failed("the item has no audio track".to_owned()))?;
        let track_id = track.id;
        let time_base = track.time_base;
        let num_frames = track.num_frames;
        let duration = track.duration;

        let Some(CodecParameters::Audio(params)) = track.codec_params.clone() else {
            return Err(MediaError::Failed(
                "the item's audio track states no codec".to_owned(),
            ));
        };

        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(&params, &AudioDecoderOptions::default())
            .map_err(|error| MediaError::Failed(error.to_string()))?;

        let sample_rate = params
            .sample_rate
            .and_then(SampleRate::new)
            .ok_or_else(|| MediaError::Failed("the item states no sample rate".to_owned()))?;
        let channels = params
            .channels
            .as_ref()
            .map(|channels| channels.count())
            .and_then(|count| u16::try_from(count).ok())
            .and_then(ChannelCount::new)
            .ok_or_else(|| MediaError::Failed("the item states no channel layout".to_owned()))?;

        Ok(Decoder {
            format,
            decoder,
            track_id,
            channels,
            sample_rate,
            total_duration: track_duration(num_frames, sample_rate, time_base, duration),
            block: Vec::new(),
            read: 0,
            exhausted: false,
            skip_frames: 0,
        })
    }

    /// Reads the item's duration without keeping a decoder for it.
    ///
    /// Probing a playlist costs the header of each file and no audio at all,
    /// which is what lets a long list show its times before anything plays.
    pub(crate) fn probe_duration(path: &Path) -> Option<Duration> {
        Decoder::open(path).ok()?.total_duration()
    }

    /// Decodes packets until one yields samples, or the container ends.
    ///
    /// A packet that fails to decode is skipped rather than fatal: that is what
    /// symphonia asks for, and one corrupt frame in the middle of a track
    /// should cost that frame rather than the rest of the item.
    fn fill(&mut self) -> bool {
        loop {
            if self.exhausted {
                return false;
            }
            let packet = match self.format.next_packet() {
                Ok(Some(packet)) => packet,
                Ok(None) => {
                    self.exhausted = true;
                    return false;
                }
                Err(error) => {
                    log::debug!("cranpose-media: container ended: {error}");
                    self.exhausted = true;
                    return false;
                }
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    self.block.clear();
                    self.read = 0;
                    decoded.copy_to_vec_interleaved(&mut self.block);
                    self.drop_skipped_frames();
                    if self.read < self.block.len() {
                        return true;
                    }
                }
                Err(symphonia::core::errors::Error::DecodeError(error)) => {
                    log::debug!("cranpose-media: skipped an undecodable packet: {error}");
                }
                Err(error) => {
                    log::warn!("cranpose-media: decoding stopped: {error}");
                    self.exhausted = true;
                    return false;
                }
            }
        }
    }
}

impl Decoder {
    /// Advances past the frames a seek landed before its target.
    fn drop_skipped_frames(&mut self) {
        if self.skip_frames == 0 {
            return;
        }
        let channels = u64::from(self.channels.get());
        let available = (self.block.len() - self.read) as u64 / channels;
        let dropped = self.skip_frames.min(available);
        self.read += (dropped * channels) as usize;
        self.skip_frames -= dropped;
    }
}

/// The item's length, preferring the frame count over the container's stated
/// duration: the frame count excludes encoder delay and padding, which is what
/// a seek bar should run to. Containers that state neither leave the duration
/// unknown, and the transport shows a running position without an end.
fn track_duration(
    num_frames: Option<u64>,
    sample_rate: SampleRate,
    time_base: Option<TimeBase>,
    duration: Option<TimeBaseUnits>,
) -> Option<Duration> {
    if let Some(frames) = num_frames {
        return Some(Duration::from_secs_f64(
            frames as f64 / f64::from(sample_rate.get()),
        ));
    }
    let nanos = time_base?.calc_duration(duration?)?.as_nanos();
    Some(Duration::from_nanos(u64::try_from(nanos).ok()?))
}

impl Iterator for Decoder {
    type Item = Sample;

    #[inline]
    fn next(&mut self) -> Option<Sample> {
        if self.read >= self.block.len() && !self.fill() {
            return None;
        }
        let sample = self.block[self.read];
        self.read += 1;
        Some(sample)
    }
}

impl SampleSource for Decoder {
    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
        let time = Time::try_from_nanos_u128(position.as_nanos()).ok_or_else(|| {
            SeekError::Failed(format!("{position:?} is not a reachable position"))
        })?;
        let seeked = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|error| match error {
                symphonia::core::errors::Error::Unsupported(_) => SeekError::Unsupported,
                other => SeekError::Failed(other.to_string()),
            })?;
        self.skip_frames = seeked
            .required_ts
            .get()
            .saturating_sub(seeked.actual_ts.get())
            .max(0) as u64;
        // The packets after a seek are discontinuous with the ones before it,
        // so the decoder must forget what it was carrying and the block it
        // already produced must not be played.
        self.decoder.reset();
        self.block.clear();
        self.read = 0;
        self.exhausted = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;

    fn one() -> NonZeroU32 {
        NonZeroU32::new(1).expect("one")
    }

    fn thousand() -> NonZeroU32 {
        NonZeroU32::new(1_000).expect("a thousand")
    }

    /// Writes a 16-bit PCM WAV of `frames` frames so the decoder can be driven
    /// against a real container rather than against a mock of one.
    ///
    /// Sample `n` of channel `c` is `n * channels + c`, which makes an
    /// out-of-order or dropped frame visible in the assertion rather than
    /// merely making the totals wrong.
    fn write_wav(path: &std::path::Path, channels: u16, rate: u32, frames: u32) {
        let bytes_per_frame = u32::from(channels) * 2;
        let data_len = frames * bytes_per_frame;
        let mut wav = Vec::with_capacity(44 + data_len as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * bytes_per_frame).to_le_bytes());
        wav.extend_from_slice(&(bytes_per_frame as u16).to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        for frame in 0..frames {
            for channel in 0..u32::from(channels) {
                let value = (frame * u32::from(channels) + channel) as i16;
                wav.extend_from_slice(&value.to_le_bytes());
            }
        }
        std::fs::write(path, wav).expect("write wav");
    }

    /// A temporary file that removes itself, so a failing assertion does not
    /// leave the next run reading a stale fixture.
    struct Fixture(std::path::PathBuf);

    impl Fixture {
        fn wav(name: &str, channels: u16, rate: u32, frames: u32) -> Fixture {
            let path = std::env::temp_dir().join(format!("cranpose-media-{name}.wav"));
            write_wav(&path, channels, rate, frames);
            Fixture(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn a_wav_decodes_to_the_shape_its_header_states() {
        let fixture = Fixture::wav("shape", 2, 8_000, 800);
        let decoder = Decoder::open(&fixture.0).expect("open");
        assert_eq!(decoder.channels().get(), 2);
        assert_eq!(decoder.sample_rate().get(), 8_000);
        assert_eq!(decoder.total_duration(), Some(Duration::from_millis(100)));
    }

    #[test]
    fn every_frame_comes_back_interleaved_and_in_order() {
        let frames = 64;
        let fixture = Fixture::wav("order", 2, 8_000, frames);
        let decoder = Decoder::open(&fixture.0).expect("open");
        let samples: Vec<f32> = decoder.collect();
        assert_eq!(samples.len(), frames as usize * 2);
        // 16-bit PCM arrives scaled into -1.0..1.0, so compare the ratios
        // rather than the raw integers the fixture wrote.
        let unit = f32::from(i16::MAX);
        for (index, sample) in samples.iter().enumerate() {
            let expected = index as f32 / unit;
            assert!(
                (sample - expected).abs() < 1e-4,
                "sample {index} was {sample}, expected {expected}"
            );
        }
    }

    #[test]
    fn seeking_moves_where_the_next_samples_come_from() {
        let rate = 8_000;
        let fixture = Fixture::wav("seek", 1, rate, rate);
        let mut decoder = Decoder::open(&fixture.0).expect("open");
        decoder
            .try_seek(Duration::from_millis(500))
            .expect("this wav can seek");
        let next = decoder.next().expect("a sample after the seek");
        let expected = (rate / 2) as f32 / f32::from(i16::MAX);
        assert!(
            (next - expected).abs() < 1e-3,
            "after seeking to the halfway point the next sample was {next}, expected {expected}"
        );
    }

    #[test]
    fn probing_reads_the_duration_without_decoding_the_audio() {
        let fixture = Fixture::wav("probe", 1, 44_100, 44_100);
        assert_eq!(
            Decoder::probe_duration(&fixture.0),
            Some(Duration::from_secs(1))
        );
    }

    #[test]
    fn a_frame_count_gives_the_duration_in_seconds() {
        let rate = NonZeroU32::new(48_000).expect("rate");
        assert_eq!(
            track_duration(Some(96_000), rate, None, None),
            Some(Duration::from_secs(2))
        );
    }

    #[test]
    fn no_frame_count_and_no_timebase_is_an_unknown_duration() {
        let rate = NonZeroU32::new(44_100).expect("rate");
        assert_eq!(
            track_duration(None, rate, None, Some(TimeBaseUnits::new(1_000))),
            None
        );
    }

    #[test]
    fn a_timebase_duration_is_used_when_no_frame_count_is_stated() {
        let rate = NonZeroU32::new(44_100).expect("rate");
        let time_base = TimeBase::new(one(), thousand());
        assert_eq!(
            track_duration(None, rate, Some(time_base), Some(TimeBaseUnits::new(2_500))),
            Some(Duration::from_millis(2_500))
        );
    }

    #[test]
    fn the_frame_count_wins_over_the_stated_duration() {
        let rate = NonZeroU32::new(1_000).expect("rate");
        let time_base = TimeBase::new(one(), thousand());
        // The container claims ten seconds; the playable frames are one.
        assert_eq!(
            track_duration(
                Some(1_000),
                rate,
                Some(time_base),
                Some(TimeBaseUnits::new(10_000))
            ),
            Some(Duration::from_secs(1))
        );
    }

    #[test]
    fn opening_something_that_is_not_media_fails_rather_than_panicking() {
        let path = std::env::temp_dir().join("cranpose-media-not-audio.bin");
        std::fs::write(&path, b"this is not a container").expect("write");
        let opened = Decoder::open(&path);
        let _ = std::fs::remove_file(&path);
        assert!(opened.is_err());
    }

    #[test]
    fn probing_something_that_is_not_media_reports_no_duration() {
        let path = std::env::temp_dir().join("cranpose-media-not-audio-probe.bin");
        std::fs::write(&path, b"this is not a container either").expect("write");
        let duration = Decoder::probe_duration(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(duration, None);
    }
}
