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
fn a_remote_uri_hints_the_extension_from_its_path() {
    assert_eq!(
        extension_of("https://host/music/track.MP3"),
        Some("mp3".to_owned())
    );
    assert_eq!(
        extension_of("https://host/track.flac?token=1&x=2"),
        Some("flac".to_owned())
    );
    assert_eq!(
        extension_of("https://host/track.ogg#start"),
        Some("ogg".to_owned())
    );
    assert_eq!(extension_of("https://host/stream"), None);
    assert_eq!(extension_of("https://host/v1.2/stream"), None);
}

#[test]
fn a_local_uri_still_hints_the_extension_from_its_path() {
    assert_eq!(
        extension_of("file:///music/track.wav"),
        Some("wav".to_owned())
    );
    assert_eq!(extension_of("/music/track.wav"), Some("wav".to_owned()));
    assert_eq!(extension_of("file:///music/track"), None);
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
