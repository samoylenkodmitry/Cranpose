//! Decode a `.opus` file back to WAV.
//!
//!   cargo run --release --example decode -- input.opus output.wav [rate]
//!
//! Opus always decodes at a rate you choose, not one stored in the file, so the
//! output rate is an argument. `OpusHead::input_sample_rate` records what the
//! original was, and is used as the default.
//!
//! A decoded Opus stream is longer than the audio that went into it at both
//! ends, and `Trim` takes both ends off: the encoder delay at the front and the
//! final granule's end-trim at the back. Real `opusenc` files carry the second
//! one, so leaving it out appends up to a frame of padding past the end of the
//! audio — silent on a clip played once, an audible gap at the seam of a loop.

#[path = "common/wav.rs"]
mod wav;

use opus_pure::{MAX_PACKET_SAMPLES, OggOpusReader, Trim};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: {} <input.opus> <output.wav> [output_rate]", args[0]);
        std::process::exit(2);
    }

    let file = std::fs::File::open(&args[1])?;
    let mut reader = OggOpusReader::new(std::io::BufReader::new(file))?;
    let head = reader.head().clone();
    let channels = head.channel_count as usize;

    // Opus decodes to 8/12/16/24/48 kHz only; fall back to 48 kHz when the file
    // records something else (or nothing).
    let requested: i32 = args
        .get(3)
        .map(|s| s.parse())
        .transpose()?
        .unwrap_or(head.input_sample_rate as i32);
    let rate = match requested {
        8_000 | 12_000 | 16_000 | 24_000 | 48_000 => requested,
        other => {
            eprintln!("note: {other} Hz is not an Opus decode rate; using 48000 Hz");
            48_000
        }
    };

    println!(
        "{}: {channels} ch, pre-skip {} samples, gain {:+.2} dB, vendor {:?}",
        args[1],
        head.pre_skip,
        head.output_gain_db(),
        reader.tags().vendor,
    );
    for comment in &reader.tags().comments {
        println!("  {comment}");
    }

    // `decoder` carries the channel count and the header's output gain, which
    // RFC 7845 §5.1 says a player should apply and which is silent when it is
    // missed: the file simply plays at the wrong level.
    let mut decoder = head.decoder(rate)?;
    let mut trim = Trim::new(&head, rate, channels)?;

    // Sized for the longest packet Opus allows, so the loop does not care what
    // frame size the file was made with. `decode` returns what it produced.
    let mut block = vec![0.0f32; MAX_PACKET_SAMPLES * channels];
    let mut samples = Vec::new();
    let mut packets = 0usize;

    for packet in reader.packets() {
        let packet = packet?;
        let n = decoder.decode(&packet.data, MAX_PACKET_SAMPLES, &mut block)?;
        samples.extend_from_slice(trim.keep(&packet, &block[..n * channels]));
        packets += 1;
    }

    let per_channel = trim.samples_emitted();
    wav::write(
        &args[2],
        &wav::Wav {
            sample_rate: rate as u32,
            channels: head.channel_count as u16,
            samples,
        },
    )?;
    println!(
        "  {packets} packets -> {} ({rate} Hz, {per_channel} samples/ch = {:.2} s of audio)",
        args[2],
        per_channel as f64 / rate as f64,
    );
    Ok(())
}
