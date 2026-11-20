//! What the frame loop shares with the audio thread: the listener, the emitters and the pause.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use super::{
    audio_listener::AudioListener,
    distance_attenuation::DistanceAttenuation,
    emitter::{Emitter, EmitterId},
    output_tap::OutputTap,
    seq_cell::SeqCell,
};

/// How many emitters a scene holds at once.
pub const MAX_EMITTERS: usize = 32;

/// What the audio thread finds when it reads an emitter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EmitterReading {
    /// The emitter ended, or another emitter took its slot.
    Gone,
    /// A write overlapped the read. The reader keeps what it read before.
    Overlapped,
    /// The emitter's place and curve.
    Placed {
        /// The place, in meters.
        position: [f32; 3],
        /// How the level falls with the distance from the listener.
        attenuation: DistanceAttenuation,
    },
}

struct Slot {
    // Odd while an emitter holds the slot, even while the slot is free
    generation: AtomicU32,
    // The position, then the four words of the curve
    state: SeqCell<7>,
}

/// The state that the frame loop and the audio thread share for spatial sound: the pose of the
/// listener's head, the places of the emitters and whether the app is paused. The player of an
/// audio backend makes one and gives a handle to it through
/// [`AudioPlayer::spatial_scene`](crate::AudioPlayer::spatial_scene).
///
/// Every call is lock-free. The frame loop writes with [`set_listener`](Self::set_listener) and
/// [`Emitter::place`]. The audio thread reads with [`listener`](Self::listener) and
/// [`read_emitter`](Self::read_emitter). The head's pose reaches the audio thread in a cell of
/// atomic words that the callback reads without a lock.
pub struct SpatialScene {
    listener: SeqCell<7>,
    slots: [Slot; MAX_EMITTERS],
    paused: AtomicBool,
    tap: OutputTap,
}

impl Default for SpatialScene {
    fn default() -> Self {
        Self::new()
    }
}

impl SpatialScene {
    /// A scene with the listener at the origin, no emitters and no pause.
    pub fn new() -> Self {
        Self {
            listener: SeqCell::new(AudioListener::AT_ORIGIN.to_words()),
            slots: std::array::from_fn(|_| Slot {
                generation: AtomicU32::new(0),
                state: SeqCell::new([0; 7]),
            }),
            paused: AtomicBool::new(false),
            tap: OutputTap::new(),
        }
    }

    /// Sets the pose of the listener's head.
    pub fn set_listener(&self, listener: AudioListener) {
        self.listener.write(listener.to_words());
    }

    /// The pose of the listener's head, or `None` while a write overlaps the read.
    pub fn listener(&self) -> Option<AudioListener> {
        self.listener.read().map(AudioListener::from_words)
    }

    /// Starts an emitter at `position` with the curve `attenuation`, or returns `None` while
    /// [`MAX_EMITTERS`] emitters exist.
    pub fn add_emitter(
        self: &Arc<Self>,
        position: [f32; 3],
        attenuation: DistanceAttenuation,
    ) -> Option<Emitter> {
        for (index, slot) in (0..=u8::MAX).zip(&self.slots) {
            let free = slot.generation.load(Ordering::Relaxed);
            if free & 1 == 1 {
                continue;
            }
            let taken = free.wrapping_add(1);
            // The place is in the slot before a voice can name the emitter
            if slot
                .generation
                .compare_exchange(free, taken, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                let id = EmitterId {
                    slot: index,
                    generation: taken,
                };
                self.place_emitter(id, position, attenuation);
                return Some(Emitter::new(Arc::clone(self), id, attenuation));
            }
        }
        None
    }

    pub(crate) fn place_emitter(
        &self,
        id: EmitterId,
        position: [f32; 3],
        attenuation: DistanceAttenuation,
    ) {
        let Some(slot) = self.slots.get(usize::from(id.slot)) else {
            return;
        };
        let [curve0, curve1, curve2, curve3] = attenuation.to_words();
        let [x, y, z] = position.map(f32::to_bits);
        slot.state.write([x, y, z, curve0, curve1, curve2, curve3]);
    }

    pub(crate) fn release_emitter(&self, id: EmitterId) {
        if let Some(slot) = self.slots.get(usize::from(id.slot)) {
            slot.generation
                .store(id.generation.wrapping_add(1), Ordering::Release);
        }
    }

    /// Reads the emitter `id`.
    pub fn read_emitter(&self, id: EmitterId) -> EmitterReading {
        let Some(slot) = self.slots.get(usize::from(id.slot)) else {
            return EmitterReading::Gone;
        };
        if slot.generation.load(Ordering::Acquire) != id.generation {
            return EmitterReading::Gone;
        }
        match slot.state.read() {
            Some([x, y, z, curve0, curve1, curve2, curve3]) => EmitterReading::Placed {
                position: [x, y, z].map(f32::from_bits),
                attenuation: DistanceAttenuation::from_words([curve0, curve1, curve2, curve3]),
            },
            None => EmitterReading::Overlapped,
        }
    }

    /// Pauses or resumes the voices that play from emitters. A paused voice fades out within one
    /// render call and then stands still, so it goes on from where it stopped. Sound that does not
    /// play from an emitter is the app's own to pause.
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    /// Whether the voices of emitters are paused.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// The debug tap on the mixed output of the backend that made this scene.
    pub const fn tap(&self) -> &OutputTap {
        &self.tap
    }
}

#[cfg(test)]
#[path = "tests/spatial_scene_tests.rs"]
mod tests;
