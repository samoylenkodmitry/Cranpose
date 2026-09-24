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
    http::{self, HttpSource},
    source::{ChannelCount, Sample, SampleRate, SampleSource, SeekError, SourceCancel},
    spool::Spool,
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
    pub(crate) fn open(uri: &str) -> Result<(Decoder, SourceCancel), MediaError> {
        let (media, cancel) = open_media(uri)?;
        Ok((Decoder::from_media(media, uri)?, cancel))
    }

    fn from_media(media: Box<dyn MediaSource>, uri: &str) -> Result<Decoder, MediaError> {
        let stream = MediaSourceStream::new(media, Default::default());

        let mut hint = Hint::new();
        if let Some(extension) = extension_of(uri) {
            hint.with_extension(&extension);
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
            .map(symphonia::core::audio::Channels::count)
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
        let media: Box<dyn MediaSource> = if http::is_http_uri(uri) {
            Box::new(HttpSource::open(uri).ok()?.0)
        } else {
            Box::new(std::fs::File::open(cranpose_services::path_from_uri(uri)?).ok()?)
        };
        Decoder::from_media(media, uri).ok()?.total_duration()
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

fn extension_of(uri: &str) -> Option<String> {
    if http::is_http_uri(uri) {
        let (_, rest) = uri.split_once("://")?;
        let path = rest.split(['?', '#']).next()?;
        let (_, name) = path.rsplit_once('/')?;
        let (_, extension) = name.rsplit_once('.')?;
        return (!extension.is_empty()).then(|| extension.to_ascii_lowercase());
    }
    cranpose_services::path_from_uri(uri)
        .as_deref()
        .and_then(Path::extension)
        .and_then(|extension| extension.to_str())
        .map(str::to_owned)
}

fn open_media(uri: &str) -> Result<(Box<dyn MediaSource>, SourceCancel), MediaError> {
    if http::is_http_uri(uri) {
        let (source, cancel) = HttpSource::open(uri)?;
        return Ok((Box::new(source), cancel));
    }
    let handle = cranpose_services::open_media_source(uri).map_err(|error| {
        if error.kind() == std::io::ErrorKind::Unsupported {
            MediaError::UnsupportedSource(uri.to_owned())
        } else {
            MediaError::Failed(format!("{uri}: {error}"))
        }
    })?;
    let mut file = handle.stream;
    if seeks(&mut file) {
        return Ok((Box::new(file), SourceCancel::default()));
    }
    log::debug!(
        "cranpose-media: {uri} does not seek; spooling {} bytes",
        handle
            .len
            .map_or_else(|| "an unstated number of".to_owned(), |len| len.to_string())
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
#[path = "tests/decode_tests.rs"]
mod tests;
