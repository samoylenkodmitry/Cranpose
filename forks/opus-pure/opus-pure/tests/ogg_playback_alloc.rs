//! A playback loop over an `.opus` file touches the heap only while warming up.
//!
//! The loop is the one a game's music player runs: read a packet into a kept
//! `OggPacket`, decode it into a kept block, trim it, and at the end of the
//! stream rewind the reader, reset the decoder and replace the `Trim`. It runs
//! on a thread with a deadline, where an allocation is a lock and a possible
//! page fault, so once the first pass has sized every buffer, the passes after
//! it must allocate nothing.
//!
//! This is its own test binary because it replaces the global allocator, with
//! one counting only while this thread has armed it.

mod common;
use common::*;
use opus_pure::{
    Application, MAX_PACKET_BYTES, MAX_PACKET_SAMPLES, OggOpusReader, OggOpusWriter, OggPacket,
    OpusEncoder, OpusHead, Trim,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Counting;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
}

fn note() {
    if ARMED.try_with(Cell::get).unwrap_or(false) {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    }
}

// SAFETY: every call forwards to `System` unchanged; counting touches only an
// atomic and a const-initialised thread local, neither of which allocates.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Allocations `f` makes on this thread.
fn allocations_in(f: impl FnOnce()) -> usize {
    let before = ALLOCATIONS.load(Ordering::Relaxed);
    ARMED.with(|a| a.set(true));
    f();
    ARMED.with(|a| a.set(false));
    ALLOCATIONS.load(Ordering::Relaxed) - before
}

/// `pcm` encoded at `rate` and `bitrate` into an Ogg Opus stream.
fn encode(rate: i32, channels: usize, bitrate: i32, pcm: &[f32]) -> Vec<u8> {
    let frame = (rate / 50) as usize;
    let mut encoder = OpusEncoder::new(rate, channels, Application::Audio).unwrap();
    encoder.bitrate_bps = bitrate;
    let head = OpusHead::for_encoder(&encoder, rate as u32);
    let mut w = OggOpusWriter::new(Vec::new(), head).unwrap();
    let mut packet = vec![0u8; MAX_PACKET_BYTES];
    for block in pcm.chunks_exact(frame * channels) {
        let n = encoder.encode(block, frame, &mut packet).unwrap();
        w.write_packet(&packet[..n]).unwrap();
    }
    w.finish().unwrap()
}

/// A looping music player's state, kept across passes.
struct Player<'a> {
    reader: OggOpusReader<std::io::Cursor<&'a [u8]>>,
    decoder: opus_pure::OpusDecoder,
    trim: Trim,
    packet: OggPacket,
    block: Vec<f32>,
    channels: usize,
}

impl<'a> Player<'a> {
    const RATE: i32 = 48_000;

    fn new(bytes: &'a [u8]) -> Self {
        let reader = OggOpusReader::new(std::io::Cursor::new(bytes)).unwrap();
        let channels = reader.head().channel_count as usize;
        Player {
            decoder: reader.head().decoder(Self::RATE).unwrap(),
            trim: Trim::new(reader.head(), Self::RATE, channels).unwrap(),
            reader,
            packet: OggPacket::default(),
            block: vec![0.0; MAX_PACKET_SAMPLES * channels],
            channels,
        }
    }

    /// Play the stream through once and go back to its start, returning how
    /// many samples the pass kept.
    fn pass(&mut self) -> usize {
        let mut kept = 0;
        while self.reader.read_packet_into(&mut self.packet).unwrap() {
            let n = self
                .decoder
                .decode(&self.packet.data, MAX_PACKET_SAMPLES, &mut self.block)
                .unwrap();
            kept += self.trim.keep_range(&self.packet, n * self.channels).len();
        }
        self.reader.rewind().unwrap();
        self.decoder.reset_state().unwrap();
        self.trim = Trim::new(self.reader.head(), Self::RATE, self.channels).unwrap();
        kept
    }
}

#[test]
fn a_looping_playback_allocates_nothing_after_its_first_pass() {
    // CELT at a music bitrate, SILK at a narrowband speech bitrate, and the
    // hybrid of the two between them.
    let second = |rate: i32| rate as usize;
    let streams = [
        (
            "CELT stereo",
            encode(
                48_000,
                2,
                128_000,
                &interleave(&[
                    music_like(48_000, second(48_000)),
                    speech_like(48_000, second(48_000)),
                ]),
            ),
        ),
        (
            "SILK",
            encode(16_000, 1, 12_000, &speech_like(16_000, second(16_000))),
        ),
        (
            "hybrid",
            encode(48_000, 1, 24_000, &speech_like(48_000, second(48_000))),
        ),
    ];
    for (what, bytes) in &streams {
        let mut player = Player::new(bytes);
        let first = player.pass();
        assert!(first > 0);

        let mut later = [0; 2];
        let allocations = allocations_in(|| later = [player.pass(), player.pass()]);
        assert_eq!(later, [first, first], "{what}");
        assert_eq!(allocations, 0, "{what} allocated on a warm pass");
    }
}
