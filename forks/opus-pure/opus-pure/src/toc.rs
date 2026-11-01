//! The table-of-contents byte that opens every Opus packet (RFC 6716 §3.1).
//!
//! One byte encodes the coding mode, audio bandwidth, frame duration and
//! channel count, plus how many frames the packet carries.

use crate::config::{Bandwidth, OpusMode};

/// The frame duration, expressed the way the TOC byte and the mode decisions
/// want it: frames per second, truncated. `None` if `frame_size` is not one of
/// the durations RFC 6716 §2.1.4 defines.
///
/// Four of the six durations divide the sample rate evenly, so the truncation is
/// exact for them. 60 ms never does (48000 / 2880 = 16.67), and the truncated 16
/// is what both `gen_toc`'s period search and libopus's own `st->Fs/frame_size`
/// run on, so it is the value to return rather than a rounded one.
pub(crate) fn frame_rate_from_params(sampling_rate: i32, frame_size: usize) -> Option<i32> {
    let frame_size = i32::try_from(frame_size).ok()?;
    // No legal duration reaches a full second, and the bound keeps the products
    // below from overflowing on a caller-supplied frame size.
    if frame_size <= 0 || frame_size > sampling_rate {
        return None;
    }
    // 2.5, 5, 10, 20, 40 and 60 ms, written the way libopus checks them
    // (opus_encoder.c) so that 60 ms needs no special case for the rates it
    // does not divide.
    let legal = 400 * frame_size == sampling_rate
        || 200 * frame_size == sampling_rate
        || 100 * frame_size == sampling_rate
        || 50 * frame_size == sampling_rate
        || 25 * frame_size == sampling_rate
        || 50 * frame_size == 3 * sampling_rate;
    if !legal {
        return None;
    }
    Some(sampling_rate / frame_size)
}

pub(crate) fn gen_toc(
    mode: OpusMode,
    frame_rate: i32,
    bandwidth: Bandwidth,
    channels: usize,
) -> u8 {
    let mut rate = frame_rate;
    let mut period = 0;
    while rate < 400 {
        rate <<= 1;
        period += 1;
    }

    let mut toc = match mode {
        OpusMode::SilkOnly => {
            let bw = (bandwidth as i32 - Bandwidth::Narrowband as i32) << 5;
            let per = (period - 2) << 3;
            (bw | per) as u8
        }
        OpusMode::CeltOnly => {
            let mut tmp = bandwidth as i32 - Bandwidth::Mediumband as i32;
            if tmp < 0 {
                tmp = 0;
            }
            let per = period << 3;
            (0x80 | (tmp << 5) | per) as u8
        }
        OpusMode::Hybrid => {
            let base_config = if bandwidth == Bandwidth::Superwideband {
                12
            } else {
                14
            };
            let period_offset = if frame_rate >= 100 { 0 } else { 1 };
            ((base_config + period_offset) << 3) as u8
        }
    };

    if channels == 2 {
        toc |= 0x04;
    }
    toc
}

pub(crate) fn mode_from_toc(toc: u8) -> OpusMode {
    if toc & 0x80 != 0 {
        OpusMode::CeltOnly
    } else if toc & 0x60 == 0x60 {
        OpusMode::Hybrid
    } else {
        OpusMode::SilkOnly
    }
}

/// The CELT end band a given audio bandwidth codes up to (libopus
/// `opus_decoder.c` / `opus_encoder.c`, `CELT_SET_END_BAND`).
///
/// Encoder and decoder must pick the same band range for a packet, so this is
/// the single definition both sides call rather than a match transcribed on
/// each side.
pub(crate) fn celt_endband_for_bandwidth(bw: Bandwidth) -> usize {
    match bw {
        Bandwidth::Narrowband => 13,
        Bandwidth::Mediumband | Bandwidth::Wideband => 17,
        Bandwidth::Superwideband => 19,
        _ => 21,
    }
}

pub(crate) fn bandwidth_from_toc(toc: u8) -> Bandwidth {
    let mode = mode_from_toc(toc);
    match mode {
        OpusMode::SilkOnly => {
            let bw_bits = (toc >> 5) & 0x03;
            match bw_bits {
                0 => Bandwidth::Narrowband,
                1 => Bandwidth::Mediumband,
                2 => Bandwidth::Wideband,
                _ => Bandwidth::Wideband,
            }
        }
        OpusMode::Hybrid => {
            let bw_bit = (toc >> 4) & 0x01;
            if bw_bit == 0 {
                Bandwidth::Superwideband
            } else {
                Bandwidth::Fullband
            }
        }
        OpusMode::CeltOnly => {
            let bw_bits = (toc >> 5) & 0x03;
            match bw_bits {
                0 => Bandwidth::Mediumband,
                1 => Bandwidth::Wideband,
                2 => Bandwidth::Superwideband,
                3 => Bandwidth::Fullband,
                _ => Bandwidth::Fullband,
            }
        }
    }
}

pub(crate) fn frame_duration_ms_from_toc(toc: u8) -> i32 {
    let mode = mode_from_toc(toc);
    match mode {
        OpusMode::SilkOnly => {
            let config = (toc >> 3) & 0x03;
            match config {
                0 => 10,
                1 => 20,
                2 => 40,
                3 => 60,
                _ => 20,
            }
        }
        OpusMode::Hybrid => {
            let config = (toc >> 3) & 0x01;
            if config == 0 { 10 } else { 20 }
        }
        OpusMode::CeltOnly => {
            let config = (toc >> 3) & 0x03;
            match config {
                0 => 2,
                1 => 5,
                2 => 10,
                3 => 20,
                _ => 20,
            }
        }
    }
}

pub(crate) fn channels_from_toc(toc: u8) -> usize {
    if toc & 0x04 != 0 { 2 } else { 1 }
}
