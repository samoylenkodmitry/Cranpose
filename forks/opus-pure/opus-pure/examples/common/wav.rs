#![allow(dead_code)]
//! Minimal 16-bit PCM WAV reader/writer shared by the examples.
//!
//! Deliberately small: enough for the examples to be runnable end to end without
//! pulling in a dependency, and not a general-purpose WAV library.

use std::io::{Read, Write};

pub struct Wav {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved samples in [-1, 1).
    pub samples: Vec<f32>,
}

pub fn read(path: &str) -> Result<Wav, String> {
    let mut buf = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| format!("{path}: {e}"))?
        .read_to_end(&mut buf)
        .map_err(|e| format!("{path}: {e}"))?;

    if buf.len() < 44 || &buf[0..4] != b"RIFF" || &buf[8..12] != b"WAVE" {
        return Err(format!("{path}: not a RIFF/WAVE file"));
    }

    let (mut channels, mut sample_rate, mut bits) = (0u16, 0u32, 0u16);
    let (mut data_off, mut data_len) = (0usize, 0usize);

    // Walk the chunk list rather than assuming a canonical 44-byte header.
    let mut i = 12usize;
    while i + 8 <= buf.len() {
        let id = &buf[i..i + 4];
        let size = u32::from_le_bytes(buf[i + 4..i + 8].try_into().unwrap()) as usize;
        let body = i + 8;
        match id {
            b"fmt " if size >= 16 && body + 16 <= buf.len() => {
                channels = u16::from_le_bytes(buf[body + 2..body + 4].try_into().unwrap());
                sample_rate = u32::from_le_bytes(buf[body + 4..body + 8].try_into().unwrap());
                bits = u16::from_le_bytes(buf[body + 14..body + 16].try_into().unwrap());
            }
            b"data" => {
                data_off = body;
                data_len = size.min(buf.len().saturating_sub(body));
            }
            _ => {}
        }
        i = body + size + (size & 1); // chunks are word-aligned
    }

    if channels == 0 || sample_rate == 0 || data_len == 0 {
        return Err(format!("{path}: missing fmt or data chunk"));
    }
    if bits != 16 {
        return Err(format!(
            "{path}: only 16-bit PCM is supported, found {bits}-bit"
        ));
    }

    let samples = buf[data_off..data_off + data_len]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b) as f32 / 32768.0)
        .collect();
    Ok(Wav {
        sample_rate,
        channels,
        samples,
    })
}

pub fn write(path: &str, wav: &Wav) -> Result<(), String> {
    let bytes_per_sample = 2u32;
    let data_len = (wav.samples.len() as u32) * bytes_per_sample;
    let byte_rate = wav.sample_rate * wav.channels as u32 * bytes_per_sample;
    let block_align = wav.channels * bytes_per_sample as u16;

    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&wav.channels.to_le_bytes());
    out.extend_from_slice(&wav.sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &s in &wav.samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }

    std::fs::File::create(path)
        .and_then(|mut f| f.write_all(&out))
        .map_err(|e| format!("{path}: {e}"))
}
