use super::*;

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
