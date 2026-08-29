use std::{io::Seek, path::Path, time::Duration};

use cranpose_services::MediaError;
use symphonia::core::{
    codecs::{
        CodecParameters,
        audio::{AudioDecoder, AudioDecoderOptions},
    },
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType, probe::Hint},
    io::{MediaSource, MediaSourceStream},
    meta::MetadataOptions,
    units::{Duration as TimeBaseUnits, Time, TimeBase},
};

use crate::{
    source::{ChannelCount, Sample, SampleRate, SampleSource, SeekError},
    spool::{Spool, SpoolCancel},
};

const SPOOL_DIRECTORY: &str = "cranpose-media-spool";

pub(crate) struct Decoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    channels: ChannelCount,
    sample_rate: SampleRate,
    total_duration: Option<Duration>,
    block: Vec<Sample>,
    read: usize,
    exhausted: bool,
    skip_frames: u64,
}

impl Decoder {
    pub(crate) fn open(uri: &str) -> Result<(Decoder, SpoolCancel), MediaError> {
        let (media, cancel) = open_media(uri)?;
        Ok((Decoder::from_media(media, uri)?, cancel))
    }

    fn from_media(media: Box<dyn MediaSource>, uri: &str) -> Result<Decoder, MediaError> {
        let stream = MediaSourceStream::new(media, Default::default());

        let mut hint = Hint::new();
        if let Some(extension) = cranpose_services::path_from_uri(uri)
            .as_deref()
            .and_then(Path::extension)
            .and_then(|extension| extension.to_str())
        {
            hint.with_extension(extension);
        }

        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                stream,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .map_err(|error| MediaError::Failed(format!("{uri}: {error}")))?;

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

    pub(crate) fn probe_duration(uri: &str) -> Option<Duration> {
        let path = cranpose_services::path_from_uri(uri)?;
        let file = std::fs::File::open(path).ok()?;
        Decoder::from_media(Box::new(file), uri)
            .ok()?
            .total_duration()
    }

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
        self.decoder.reset();
        self.block.clear();
        self.read = 0;
        self.exhausted = false;
        Ok(())
    }
}

fn open_media(uri: &str) -> Result<(Box<dyn MediaSource>, SpoolCancel), MediaError> {
    let handle = cranpose_services::open_media_source(uri).map_err(|error| {
        if error.kind() == std::io::ErrorKind::Unsupported {
            MediaError::UnsupportedSource(uri.to_owned())
        } else {
            MediaError::Failed(format!("{uri}: {error}"))
        }
    })?;
    let mut file = handle.stream;
    if seeks(&mut file) {
        return Ok((Box::new(file), SpoolCancel::default()));
    }
    log::debug!(
        "cranpose-media: {uri} does not seek; spooling {} bytes",
        handle
            .len
            .map(|len| len.to_string())
            .unwrap_or_else(|| "an unstated number of".to_owned())
    );
    let (spool, cancel) = Spool::start(Box::new(file), &spool_directory()?, handle.len)
        .map_err(|error| MediaError::Failed(format!("{uri}: no spool: {error}")))?;
    Ok((Box::new(spool), cancel))
}

fn seeks(file: &mut std::fs::File) -> bool {
    file.stream_position().is_ok()
}

fn spool_directory() -> Result<std::path::PathBuf, MediaError> {
    let directories = cranpose_services::application_directories()
        .map_err(|error| MediaError::Failed(format!("no cache directory: {error}")))?;
    Ok(directories.cache.join(SPOOL_DIRECTORY))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    fn one() -> NonZeroU32 {
        NonZeroU32::new(1).expect("one")
    }

    fn thousand() -> NonZeroU32 {
        NonZeroU32::new(1_000).expect("a thousand")
    }

    fn write_wav(path: &std::path::Path, channels: u16, rate: u32, frames: u32) {
        let bytes_per_frame = u32::from(channels) * 2;
        let data_len = frames * bytes_per_frame;
        let mut wav = Vec::with_capacity(44 + data_len as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
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

    fn scratch(tag: &str) -> std::path::PathBuf {
        cranpose_core::test_scratch_dir(env!("CARGO_MANIFEST_DIR"), tag)
    }

    struct Fixture(std::path::PathBuf);

    impl Fixture {
        fn wav(name: &str, channels: u16, rate: u32, frames: u32) -> Fixture {
            let path = scratch(name).join(format!("{name}.wav"));
            write_wav(&path, channels, rate, frames);
            Fixture(path)
        }

        fn uri(&self) -> String {
            cranpose_services::uri_for_path(&self.0)
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
        let (decoder, _cancel) = Decoder::open(&fixture.uri()).expect("open");
        assert_eq!(decoder.channels().get(), 2);
        assert_eq!(decoder.sample_rate().get(), 8_000);
        assert_eq!(decoder.total_duration(), Some(Duration::from_millis(100)));
    }

    #[test]
    fn every_frame_comes_back_interleaved_and_in_order() {
        let frames = 64;
        let fixture = Fixture::wav("order", 2, 8_000, frames);
        let (decoder, _cancel) = Decoder::open(&fixture.uri()).expect("open");
        let samples: Vec<f32> = decoder.collect();
        assert_eq!(samples.len(), frames as usize * 2);
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
        let (mut decoder, _cancel) = Decoder::open(&fixture.uri()).expect("open");
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
            Decoder::probe_duration(&fixture.uri()),
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
    fn a_document_uri_reports_no_duration_without_being_opened() {
        assert_eq!(Decoder::probe_duration("content://example/track"), None);
    }

    #[test]
    fn a_uri_with_no_platform_opener_is_unsupported() {
        assert!(matches!(
            Decoder::open("content://example/track").map(|opened| opened.0),
            Err(MediaError::UnsupportedSource(uri)) if uri == "content://example/track"
        ));
    }

    #[test]
    fn opening_something_that_is_not_media_fails_rather_than_panicking() {
        let path = scratch("not-audio").join("not-audio.bin");
        std::fs::write(&path, b"this is not a container").expect("write");
        let opened = Decoder::open(&cranpose_services::uri_for_path(&path)).map(|opened| opened.0);
        let _ = std::fs::remove_file(&path);
        assert!(opened.is_err());
    }

    #[test]
    fn probing_something_that_is_not_media_reports_no_duration() {
        let path = scratch("not-audio-probe").join("not-audio-probe.bin");
        std::fs::write(&path, b"this is not a container either").expect("write");
        let duration = Decoder::probe_duration(&cranpose_services::uri_for_path(&path));
        let _ = std::fs::remove_file(&path);
        assert_eq!(duration, None);
    }
}
