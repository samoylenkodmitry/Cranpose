//! RIFF/WAVE decoding for [`AudioClip`](super::AudioClip).
//!
//! Game cue banks ship as uncompressed WAV, so the framework decodes that
//! container itself instead of pulling a codec crate into every build. The
//! decoder is pure arithmetic over a byte slice: no allocation beyond the
//! sample buffer it produces, no panics on malformed input, and every length
//! is checked before it is used.
//!
//! Supported: PCM 8/16/24/32-bit integer and IEEE 32/64-bit float, any sample
//! rate, any channel count (three or more channels are downmixed to mono).
//! Compressed payloads (ADPCM, MP3-in-WAV) are reported as
//! [`AudioError::UnsupportedFormat`](super::AudioError::UnsupportedFormat).

use super::{AudioClip, AudioError};

const FORMAT_PCM: u16 = 1;
const FORMAT_IEEE_FLOAT: u16 = 3;
const FORMAT_EXTENSIBLE: u16 = 0xfffe;

/// Whether `bytes` opens with the RIFF/WAVE magic this decoder understands.
pub(super) fn is_wav(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE"
}

/// Decodes a RIFF/WAVE byte stream into interleaved `f32` samples.
pub(super) fn decode(bytes: &[u8]) -> Result<AudioClip, AudioError> {
    if !is_wav(bytes) {
        return Err(AudioError::UnsupportedFormat(
            "not a RIFF/WAVE stream".to_string(),
        ));
    }

    let mut format: Option<WaveFormat> = None;
    let mut data: Option<&[u8]> = None;
    let mut offset = 12usize;

    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = read_u32(bytes, offset + 4)? as usize;
        let body_start = offset + 8;
        let body_end = body_start.saturating_add(size).min(bytes.len());
        let body = &bytes[body_start..body_end];

        if id == b"fmt " {
            format = Some(parse_format(body)?);
        } else if id == b"data" {
            data = Some(body);
        }

        // Chunks are word aligned: an odd payload is followed by a pad byte.
        let advance = size.saturating_add(size & 1);
        match body_start.checked_add(advance) {
            Some(next) if next > offset => offset = next,
            _ => break,
        }
    }

    let format = format.ok_or_else(|| AudioError::Decode("WAVE stream has no fmt chunk".into()))?;
    let data = data.ok_or_else(|| AudioError::Decode("WAVE stream has no data chunk".into()))?;

    let samples = decode_samples(&format, data)?;
    AudioClip::from_samples(samples, output_channels(format.channels), format.sample_rate)
}

/// Mono and stereo stay as recorded; three or more channels fold to mono.
fn output_channels(channels: u16) -> u16 {
    if channels > 2 {
        1
    } else {
        channels
    }
}

struct WaveFormat {
    tag: u16,
    channels: u16,
    sample_rate: u32,
    bits_per_sample: u16,
}

fn parse_format(body: &[u8]) -> Result<WaveFormat, AudioError> {
    if body.len() < 16 {
        return Err(AudioError::Decode("WAVE fmt chunk is truncated".into()));
    }
    let mut tag = read_u16(body, 0)?;
    let channels = read_u16(body, 2)?;
    let sample_rate = read_u32(body, 4)?;
    let bits_per_sample = read_u16(body, 14)?;

    if tag == FORMAT_EXTENSIBLE {
        if body.len() < 26 {
            return Err(AudioError::Decode(
                "WAVE extensible fmt chunk is truncated".into(),
            ));
        }
        // The extension's sub-format GUID starts with the real format tag.
        tag = read_u16(body, 24)?;
    }

    if channels == 0 {
        return Err(AudioError::Decode("WAVE stream declares no channels".into()));
    }
    if sample_rate == 0 {
        return Err(AudioError::Decode(
            "WAVE stream declares a zero sample rate".into(),
        ));
    }

    Ok(WaveFormat {
        tag,
        channels,
        sample_rate,
        bits_per_sample,
    })
}

fn decode_samples(format: &WaveFormat, data: &[u8]) -> Result<Vec<f32>, AudioError> {
    let bytes_per_sample = usize::from(format.bits_per_sample).div_ceil(8);
    if bytes_per_sample == 0 {
        return Err(AudioError::Decode(
            "WAVE stream declares zero bits per sample".into(),
        ));
    }

    let decode_one: fn(&[u8]) -> f32 = match (format.tag, format.bits_per_sample) {
        (FORMAT_PCM, 8) => |chunk| (f32::from(chunk[0]) - 128.0) / 128.0,
        (FORMAT_PCM, 16) => |chunk| f32::from(i16::from_le_bytes([chunk[0], chunk[1]])) / 32_768.0,
        (FORMAT_PCM, 24) => |chunk| {
            let value = i32::from_le_bytes([0, chunk[0], chunk[1], chunk[2]]) >> 8;
            value as f32 / 8_388_608.0
        },
        (FORMAT_PCM, 32) => |chunk| {
            let value = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            value as f32 / 2_147_483_648.0
        },
        (FORMAT_IEEE_FLOAT, 32) => |chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
        (FORMAT_IEEE_FLOAT, 64) => |chunk| {
            f64::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]) as f32
        },
        (tag, bits) => {
            return Err(AudioError::UnsupportedFormat(format!(
                "WAVE format tag {tag} at {bits} bits per sample"
            )))
        }
    };

    let channels = usize::from(format.channels);
    let frame_bytes = bytes_per_sample * channels;
    let frames = data.len() / frame_bytes;
    if frames == 0 {
        return Err(AudioError::Decode("WAVE data chunk holds no frames".into()));
    }

    // Three or more channels are folded to mono: a surround cue mixed into a
    // game's stereo bus wants every channel audible, not the first two.
    let out_channels = usize::from(output_channels(format.channels));
    let mut samples = Vec::with_capacity(frames * out_channels);
    for frame in 0..frames {
        let base = frame * frame_bytes;
        if channels <= 2 {
            for channel in 0..channels {
                let start = base + channel * bytes_per_sample;
                samples.push(decode_one(&data[start..start + bytes_per_sample]));
            }
        } else {
            let mut sum = 0.0f32;
            for channel in 0..channels {
                let start = base + channel * bytes_per_sample;
                sum += decode_one(&data[start..start + bytes_per_sample]);
            }
            samples.push(sum / channels as f32);
        }
    }

    Ok(samples)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, AudioError> {
    bytes
        .get(offset..offset + 2)
        .map(|slice| u16::from_le_bytes([slice[0], slice[1]]))
        .ok_or_else(|| AudioError::Decode("WAVE stream ended inside a header field".into()))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, AudioError> {
    bytes
        .get(offset..offset + 4)
        .map(|slice| u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
        .ok_or_else(|| AudioError::Decode("WAVE stream ended inside a header field".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a minimal 16-bit PCM WAVE stream around `frames` interleaved samples.
    fn wav_pcm16(channels: u16, sample_rate: u32, frames: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = frames.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36u32 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&(sample_rate * u32::from(channels) * 2).to_le_bytes());
        out.extend_from_slice(&(channels * 2).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn decodes_mono_pcm16() {
        let bytes = wav_pcm16(1, 22_050, &[0, 16_384, -16_384, 32_767]);
        let clip = decode(&bytes).expect("decodes");
        assert_eq!(clip.channels(), 1);
        assert_eq!(clip.sample_rate(), 22_050);
        assert_eq!(clip.frames(), 4);
        assert!((clip.samples()[1] - 0.5).abs() < 1e-4);
        assert!((clip.samples()[2] + 0.5).abs() < 1e-4);
    }

    #[test]
    fn decodes_stereo_pcm16() {
        let bytes = wav_pcm16(2, 48_000, &[0, 32_767, -32_768, 0]);
        let clip = decode(&bytes).expect("decodes");
        assert_eq!(clip.channels(), 2);
        assert_eq!(clip.frames(), 2);
    }

    #[test]
    fn skips_unknown_chunks() {
        let mut bytes = wav_pcm16(1, 8_000, &[100, -100]);
        // Splice a LIST chunk with an odd payload between `fmt ` and `data`.
        let data_at = bytes
            .windows(4)
            .position(|w| w == b"data")
            .expect("data chunk present");
        let mut spliced = bytes[..data_at].to_vec();
        spliced.extend_from_slice(b"LIST");
        spliced.extend_from_slice(&3u32.to_le_bytes());
        spliced.extend_from_slice(&[1, 2, 3, 0]);
        spliced.extend_from_slice(&bytes[data_at..]);
        bytes = spliced;
        let clip = decode(&bytes).expect("decodes past unknown chunk");
        assert_eq!(clip.frames(), 2);
    }

    #[test]
    fn rejects_non_wave_bytes() {
        assert!(matches!(
            decode(b"not audio at all"),
            Err(AudioError::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn rejects_truncated_header_without_panicking() {
        for len in 0..12 {
            assert!(decode(&vec![0u8; len]).is_err());
        }
        let mut bytes = wav_pcm16(1, 8_000, &[1, 2, 3]);
        bytes.truncate(20);
        assert!(decode(&bytes).is_err());
    }

    #[test]
    fn rejects_compressed_payloads() {
        let mut bytes = wav_pcm16(1, 8_000, &[1, 2]);
        // Rewrite the format tag to IMA ADPCM.
        bytes[20] = 0x11;
        bytes[21] = 0x00;
        assert!(matches!(
            decode(&bytes),
            Err(AudioError::UnsupportedFormat(_))
        ));
    }

    #[test]
    fn downmixes_more_than_two_channels_to_mono() {
        let bytes = wav_pcm16(4, 44_100, &[32_767, 32_767, 32_767, 32_767]);
        let clip = decode(&bytes).expect("decodes");
        assert_eq!(clip.channels(), 1);
        assert_eq!(clip.frames(), 1);
        assert!((clip.samples()[0] - 1.0).abs() < 1e-3);
    }
}
