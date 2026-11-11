// The clock of a performance, shared by the thread that draws and the
// thread that renders the sound.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub(super) const SLOWEST_TEMPO: f32 = 0.25;
pub(super) const FASTEST_TEMPO: f32 = 4.0;

pub(super) struct Transport {
    playing: AtomicBool,
    tempo: AtomicU32,
    volume: AtomicU32,
    length: AtomicU32,
    position: AtomicU64,
}

impl Transport {
    pub(super) fn new() -> Self {
        Self {
            playing: AtomicBool::new(false),
            tempo: AtomicU32::new(1.0f32.to_bits()),
            volume: AtomicU32::new(1.0f32.to_bits()),
            length: AtomicU32::new(0.0f32.to_bits()),
            position: AtomicU64::new(0.0f64.to_bits()),
        }
    }

    pub(super) fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    pub(super) fn set_playing(&self, playing: bool) {
        self.playing.store(playing, Ordering::Relaxed);
    }

    // Song seconds that pass in one second of sound.
    pub(super) fn tempo(&self) -> f32 {
        f32::from_bits(self.tempo.load(Ordering::Relaxed))
    }

    pub(super) fn set_tempo(&self, tempo: f32) {
        let tempo = tempo.clamp(SLOWEST_TEMPO, FASTEST_TEMPO);
        self.tempo.store(tempo.to_bits(), Ordering::Relaxed);
    }

    pub(super) fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    pub(super) fn set_volume(&self, volume: f32) {
        self.volume
            .store(volume.max(0.0).to_bits(), Ordering::Relaxed);
    }

    // Song seconds the clock stops at.
    pub(super) fn length(&self) -> f32 {
        f32::from_bits(self.length.load(Ordering::Relaxed))
    }

    pub(super) fn set_length(&self, seconds: f32) {
        self.length
            .store(seconds.max(0.0).to_bits(), Ordering::Relaxed);
    }

    // Song seconds the renderer has reached.
    pub(super) fn position(&self) -> f64 {
        f64::from_bits(self.position.load(Ordering::Relaxed))
    }

    pub(super) fn set_position(&self, seconds: f64) {
        self.position.store(seconds.to_bits(), Ordering::Relaxed);
    }
}
