// One part of a score as a composable. Inside a roll it composes its notes
// of the roll's slice, each a `NoteBar` that is drawn and sounds. The notes
// take the voicing around the call, so `Instrument`, `Transpose` and
// `Volume` change the part they wrap: its sound, and with `Transpose` its
// place over the keys. A part that leaves the composition takes its notes
// off the roll, and they fall silent.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{
    compositionLocalOf, derivedStateOf, remember, with_key, CompositionLocal,
    CompositionLocalProvider, DisposableEffect, SideEffect, State,
};
use cranpose_ui::{composable, Color};

use super::{
    held_notes::PartSlot,
    note_bar::NoteBar,
    performance::{local_performance, Performance},
    roll::local_roll_slice,
    song::{note_seconds, Note, Notes},
    voicing::local_voicing,
};

// How many of the notes a part composes in one slice the clock has
// passed the start of, and how many it has passed the key release of.
// One of the two changes when one of those notes starts or stops to sound.
#[derive(Clone, Copy, PartialEq)]
struct Crossings {
    started: usize,
    lifted: usize,
}

// The part a note composable belongs to.
#[derive(Clone)]
pub struct PartScope {
    // The slot the performance holds the part in, once it does.
    slot: Rc<Cell<Option<PartSlot>>>,
    crossings: State<Crossings>,
    pub color: Color,
}

impl PartScope {
    pub fn slot(&self) -> Option<PartSlot> {
        self.slot.get()
    }

    // Whether the key of `note`, a note of this part and slice, is down at
    // the clock of `performance`. The part watches the clock for all of
    // its notes: a scope that calls this runs again when one of them
    // starts or lifts, and not with each move of the clock.
    pub fn sounds_now(&self, note: &Note, performance: &Performance) -> bool {
        self.crossings.get();
        let seconds = note_seconds(performance.seconds_now());
        (note.start..note.key_released_at()).contains(&seconds)
    }
}

impl PartialEq for PartScope {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.slot, &other.slot) && self.color == other.color
    }
}

// The part the caller is composed in, if it is in one.
pub fn local_part() -> CompositionLocal<Option<PartScope>> {
    thread_local! {
        static LOCAL_PART: RefCell<Option<CompositionLocal<Option<PartScope>>>> = const { RefCell::new(None) };
    }
    LOCAL_PART.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| compositionLocalOf(|| None))
            .clone()
    })
}

// Composes the notes of `notes` that sound in the slice of the roll around
// the call, in `color`. Outside a roll it composes nothing.
#[composable]
pub fn Part(notes: Notes, color: Color) {
    let (Some(slice), Some(performance)) =
        (local_roll_slice().current(), local_performance().current())
    else {
        return;
    };
    let voicing = local_voicing().current();
    let slot = remember(|| Rc::new(Cell::new(None))).with(Rc::clone);

    {
        let performance = performance.clone();
        let slot = Rc::clone(&slot);
        let notes = notes.clone();
        DisposableEffect((notes.clone(), color), move |scope| {
            slot.set(performance.hold_part(&notes, color));
            scope.on_dispose(move || {
                if let Some(held) = slot.take() {
                    performance.release_part(held);
                }
            })
        });
    }
    {
        let slot = Rc::clone(&slot);
        let performance = performance.clone();
        SideEffect(move || {
            if let Some(held) = slot.get() {
                performance.voice_part(held, voicing);
            }
        });
    }

    // The notes that sound in the slice: the ones that start in it, and
    // the ones that start before it and still ring.
    let earliest = f64::from(slice.from - notes.longest_ring());
    let first = notes.first_at_or_after(earliest);
    let end = notes.first_at_or_after(f64::from(slice.to()));
    let crossings = {
        let notes = notes.clone();
        derivedStateOf(move || {
            let seconds = note_seconds(performance.seconds());
            let composed = &notes.as_slice()[first..end];
            Crossings {
                started: composed.partition_point(|note| note.start <= seconds),
                lifted: composed
                    .iter()
                    .filter(|note| note.key_released_at() <= seconds)
                    .count(),
            }
        })
    };

    let scope = PartScope {
        slot,
        crossings,
        color,
    };
    CompositionLocalProvider([local_part().provides(Some(scope))], move || {
        for (index, note) in (first..end).zip(&notes.as_slice()[first..end]) {
            if note.silent_at() <= slice.from {
                continue;
            }
            let Ok(index) = u32::try_from(index) else {
                continue;
            };
            with_key(&index, || NoteBar(*note, index));
        }
    });
}
