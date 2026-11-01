//! Encode a WAV file to a playable `.opus` file.
//!
//!   cargo run --release --example encode -- input.wav output.opus [bitrate]
//!
//! The output is a standard Ogg Opus stream: `ffplay`, VLC and `opusdec` will
//! all open it.
//!
//! It is also *gapless*: decoded and trimmed as RFC 7845 says, it comes back as
//! exactly the samples that went in, not a frame more or less. Two things have
//! to be right for that, and neither is automatic. See "Ending the file exactly"
//! below; `examples/decode.rs` is the other half.

#[path = "common/wav.rs"]
mod wav;

use opus_pure::{Application, MAX_PACKET_BYTES, OggOpusWriter, OpusEncoder, OpusHead, OpusTags};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: {} <input.wav> <output.opus> [bitrate_bps]", args[0]);
        std::process::exit(2);
    }
    let bitrate: i32 = args
        .get(3)
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(64_000);

    let input = wav::read(&args[1])?;
    if input.samples.is_empty() {
        return Err(format!("{}: no audio samples", args[1]).into());
    }
    let channels = input.channels as usize;
    let rate = input.sample_rate as i32;

    // Opus codes 20 ms frames at the encoder's own rate. The container counts
    // everything at 48 kHz regardless, and `write_packet` reads that out of the
    // packet rather than making us convert it.
    let frame = (rate / 50) as usize;

    let mut encoder = OpusEncoder::new(rate, channels, Application::Audio)?;
    encoder.bitrate_bps = bitrate;

    let mut tags = OpusTags::new();
    tags.push("ENCODER", concat!("opus-pure ", env!("CARGO_PKG_VERSION")))?;
    // Built from the encoder, so the pre-skip is that encoder's real delay
    // rather than the conventional constant.
    let head = OpusHead::for_encoder(&encoder, input.sample_rate);

    // ---- Ending the file exactly (RFC 7845 §4.2 and §4.4) ----
    //
    // Every Opus rate divides 48 kHz, so one encoder-rate sample is this many
    // granule ticks and the conversions below are exact.
    let ticks = 48_000 / rate as usize;
    let total = input.samples.len() / channels; // sample frames of real audio
    //
    // 1. The encoder runs `pre_skip` samples behind its input, so the last
    //    `pre_skip` samples of the audio are still inside it when the input
    //    runs out. Feeding that much extra silence is what flushes them; stop
    //    at the audio and the tail is simply lost. Then round up to a whole
    //    frame, because Opus has no partial ones.
    let lookahead = (head.pre_skip as usize).div_ceil(ticks);
    let frames = (total + lookahead).div_ceil(frame);
    //
    // 2. That padding now decodes as real output, so the file has to say where
    //    the audio stopped. The final granule position is the pre-skip plus the
    //    audio and nothing else; a player trims back to it. The last packet
    //    carries the difference between that and what the writer has already
    //    counted, which is always between 1 and one frame's worth.
    let final_granule = u64::from(head.pre_skip) + (total * ticks) as u64;

    let file = std::fs::File::create(&args[2])?;
    let mut writer = OggOpusWriter::with_tags(std::io::BufWriter::new(file), head, tags)?;

    let mut packet = vec![0u8; MAX_PACKET_BYTES];
    let mut payload_bytes = 0usize;
    let per_frame = frame * channels;
    let mut block = vec![0.0f32; per_frame];

    for i in 0..frames {
        // Whole frames only: past the end of the input the block is silence,
        // which is the padding that flushes the encoder's delay.
        let start = (i * per_frame).min(input.samples.len());
        let end = (start + per_frame).min(input.samples.len());
        block[..end - start].copy_from_slice(&input.samples[start..end]);
        block[end - start..].fill(0.0);

        let n = encoder.encode(&block, frame, &mut packet)?;
        if i + 1 == frames {
            let duration = final_granule - writer.granule() as u64;
            writer.write_packet_with_duration(&packet[..n], duration as u32)?;
        } else {
            writer.write_packet(&packet[..n])?;
        }
        payload_bytes += n;
    }
    writer.finish()?;

    let secs = total as f64 / rate as f64;
    println!(
        "{} -> {}\n  {channels} ch @ {rate} Hz, {frames} frames ({secs:.2} s of audio)\n  \
         {payload_bytes} payload bytes = {:.1} kb/s (target {:.1} kb/s)",
        args[1],
        args[2],
        payload_bytes as f64 * 8.0 / secs / 1000.0,
        bitrate as f64 / 1000.0,
    );
    Ok(())
}
