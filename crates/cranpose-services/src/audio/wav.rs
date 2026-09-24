use super::{AudioClip, AudioError};

const FORMAT_PCM: u16 = 1;
const FORMAT_IEEE_FLOAT: u16 = 3;
const FORMAT_EXTENSIBLE: u16 = 0xfffe;

pub(super) fn is_wav(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE"
}

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

        let advance = size.saturating_add(size & 1);
        match body_start.checked_add(advance) {
            Some(next) if next > offset => offset = next,
            _ => break,
        }
    }

    let format = format.ok_or_else(|| AudioError::Decode("WAVE stream has no fmt chunk".into()))?;
    let data = data.ok_or_else(|| AudioError::Decode("WAVE stream has no data chunk".into()))?;

    let samples = decode_samples(&format, data)?;
    AudioClip::from_samples(
        samples,
        output_channels(format.channels),
        format.sample_rate,
    )
}

fn output_channels(channels: u16) -> u16 {
    if channels > 2 { 1 } else { channels }
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
        tag = read_u16(body, 24)?;
    }

    if channels == 0 {
        return Err(AudioError::Decode(
            "WAVE stream declares no channels".into(),
        ));
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
        (FORMAT_IEEE_FLOAT, 32) => {
            |chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
        }
        (FORMAT_IEEE_FLOAT, 64) => |chunk| {
            f64::from_le_bytes([
                chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
            ]) as f32
        },
        (tag, bits) => {
            return Err(AudioError::UnsupportedFormat(format!(
                "WAVE format tag {tag} at {bits} bits per sample"
            )));
        }
    };

    let channels = usize::from(format.channels);
    let frame_bytes = bytes_per_sample * channels;
    let frames = data.len() / frame_bytes;
    if frames == 0 {
        return Err(AudioError::Decode("WAVE data chunk holds no frames".into()));
    }

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
#[path = "tests/wav_tests.rs"]
mod tests;
