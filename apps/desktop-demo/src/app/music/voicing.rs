// How the notes inside a part of the composition sound: the composables
// here change it for their content, the way `Bpm` and `RelativeVolume` do
// in DaKapo (https://github.com/O-O-Balance/DaKapo).

use std::cell::RefCell;

use cranpose_core::{compositionLocalOf, CompositionLocal, CompositionLocalProvider};

use super::timbre::Timbre;

const LOWEST_TRANSPOSE: i8 = -24;
const HIGHEST_TRANSPOSE: i8 = 24;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Voicing {
    pub timbre: Timbre,
    // Semitones every key sounds above its own pitch.
    pub transpose: i8,
    // The share of its own loudness every strike keeps.
    pub volume: f32,
}

impl Default for Voicing {
    fn default() -> Self {
        Self {
            timbre: Timbre::Piano,
            transpose: 0,
            volume: 1.0,
        }
    }
}

pub fn local_voicing() -> CompositionLocal<Voicing> {
    thread_local! {
        static LOCAL_VOICING: RefCell<Option<CompositionLocal<Voicing>>> = const { RefCell::new(None) };
    }
    LOCAL_VOICING.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| compositionLocalOf(Voicing::default))
            .clone()
    })
}

// Each of these reads the voicing around its call and provides a changed one
// to its content. The provider is the only scope they need, so they are
// plain functions.
fn voiced(voicing: Voicing, content: impl FnMut() + 'static) {
    CompositionLocalProvider([local_voicing().provides(voicing)], content);
}

// Plays `content` with the sound of `timbre`.
#[expect(non_snake_case, reason = "a block of a score reads as a composable")]
pub fn Instrument(timbre: Timbre, content: impl FnMut() + 'static) {
    let voicing = local_voicing().current();
    voiced(Voicing { timbre, ..voicing }, content);
}

// Plays `content` this many semitones above the pitch it has outside.
#[expect(non_snake_case, reason = "a block of a score reads as a composable")]
pub fn Transpose(semitones: i8, content: impl FnMut() + 'static) {
    let voicing = local_voicing().current();
    let transpose = voicing
        .transpose
        .saturating_add(semitones)
        .clamp(LOWEST_TRANSPOSE, HIGHEST_TRANSPOSE);
    voiced(
        Voicing {
            transpose,
            ..voicing
        },
        content,
    );
}

// Plays `content` at this share of the loudness it has outside.
#[expect(non_snake_case, reason = "a block of a score reads as a composable")]
pub fn Volume(volume: f32, content: impl FnMut() + 'static) {
    let voicing = local_voicing().current();
    voiced(
        Voicing {
            volume: voicing.volume * volume.max(0.0),
            ..voicing
        },
        content,
    );
}
