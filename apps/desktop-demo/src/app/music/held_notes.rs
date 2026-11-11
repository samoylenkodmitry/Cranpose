// The parts and notes that composables hold in a player, on the thread that
// draws. A part is held once for each place it is composed in, and so is a
// note; the synth hears of the first hold and of the last release.

use cranpose_ui::Color;

use super::{
    song::{note_seconds, Note},
    synth::{MAX_NOTES, MAX_PARTS},
    voicing::Voicing,
};

// The slot of a part a player holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PartSlot(pub(super) u8);

// The place of a note a player holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotePlace(pub(super) u16);

// A key a part holds down, as a picture shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyDown {
    // The key at the pitch its part sounds it.
    pub key: u8,
    // The color of its part.
    pub color: Color,
    // Song seconds the key went down at.
    pub since: f32,
}

#[derive(Clone, Copy)]
pub(super) struct HeldPart {
    // What tells this part from the others: the identity of its notes.
    score: usize,
    holders: u32,
    color: Color,
    pub(super) voicing: Voicing,
}

#[derive(Clone, Copy)]
pub(super) struct HeldNote {
    pub(super) part: u8,
    // The place of the note among the notes of its part.
    index: u32,
    holders: u32,
    pub(super) note: Note,
}

// Whether a hold or a release changed what the synth plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Heard {
    // The first hold, or the last release: the synth hears of it.
    Changed,
    // One holder more or fewer of something the synth already plays.
    Same,
}

pub(super) struct HeldNotes {
    parts: [Option<HeldPart>; MAX_PARTS as usize],
    notes: Box<[Option<HeldNote>]>,
}

impl HeldNotes {
    pub(super) fn new() -> Self {
        Self {
            parts: [None; MAX_PARTS as usize],
            notes: vec![None; usize::from(MAX_NOTES)].into_boxed_slice(),
        }
    }

    // Holds the part with the notes of identity `score`, shown in `color`.
    // `None` when every slot is taken.
    pub(super) fn hold_part(&mut self, score: usize, color: Color) -> Option<(PartSlot, Heard)> {
        let held = (0..MAX_PARTS)
            .find(|slot| self.parts[usize::from(*slot)].is_some_and(|part| part.score == score));
        if let Some(slot) = held {
            let part = self.parts[usize::from(slot)].as_mut()?;
            part.holders += 1;
            part.color = color;
            return Some((PartSlot(slot), Heard::Same));
        }
        let slot = (0..MAX_PARTS).find(|slot| self.parts[usize::from(*slot)].is_none())?;
        self.parts[usize::from(slot)] = Some(HeldPart {
            score,
            holders: 1,
            color,
            voicing: Voicing::DEFAULT,
        });
        Some((PartSlot(slot), Heard::Changed))
    }

    pub(super) fn release_part(&mut self, slot: PartSlot) -> Heard {
        let place = &mut self.parts[usize::from(slot.0)];
        match place {
            Some(part) if part.holders > 1 => {
                part.holders -= 1;
                Heard::Same
            }
            Some(_) => {
                *place = None;
                Heard::Changed
            }
            None => Heard::Same,
        }
    }

    // Sets how the part in `slot` sounds.
    pub(super) fn voice_part(&mut self, slot: PartSlot, voicing: Voicing) -> Heard {
        match self.parts[usize::from(slot.0)].as_mut() {
            Some(part) if part.voicing != voicing => {
                part.voicing = voicing;
                Heard::Changed
            }
            _ => Heard::Same,
        }
    }

    // Holds the note at `index` of the part in `slot`. `None` when every
    // place is taken.
    pub(super) fn hold_note(
        &mut self,
        slot: PartSlot,
        index: u32,
        note: Note,
    ) -> Option<(NotePlace, Heard)> {
        let held = (0..MAX_NOTES).find(|place| {
            self.notes[usize::from(*place)]
                .is_some_and(|held| held.part == slot.0 && held.index == index)
        });
        if let Some(place) = held {
            self.notes[usize::from(place)].as_mut()?.holders += 1;
            return Some((NotePlace(place), Heard::Same));
        }
        let place = (0..MAX_NOTES).find(|place| self.notes[usize::from(*place)].is_none())?;
        self.notes[usize::from(place)] = Some(HeldNote {
            part: slot.0,
            index,
            holders: 1,
            note,
        });
        Some((NotePlace(place), Heard::Changed))
    }

    pub(super) fn release_note(&mut self, place: NotePlace) -> Heard {
        let held = &mut self.notes[usize::from(place.0)];
        match held {
            Some(note) if note.holders > 1 => {
                note.holders -= 1;
                Heard::Same
            }
            Some(_) => {
                *held = None;
                Heard::Changed
            }
            None => Heard::Same,
        }
    }

    pub(super) fn parts(&self) -> impl Iterator<Item = (PartSlot, &HeldPart)> {
        (0..MAX_PARTS)
            .zip(&self.parts)
            .filter_map(|(slot, part)| Some((PartSlot(slot), part.as_ref()?)))
    }

    pub(super) fn notes(&self) -> impl Iterator<Item = (NotePlace, &HeldNote)> {
        (0..MAX_NOTES)
            .zip(self.notes.iter())
            .filter_map(|(place, note)| Some((NotePlace(place), note.as_ref()?)))
    }

    // Calls `down` for each key the held notes keep down at `seconds` of the
    // song.
    pub(super) fn for_each_key_down(&self, seconds: f64, mut down: impl FnMut(KeyDown)) {
        let seconds = note_seconds(seconds);
        for (_, held) in self.notes() {
            let Some(part) = self.parts[usize::from(held.part)] else {
                continue;
            };
            if !(held.note.start..held.note.key_released_at()).contains(&seconds) {
                continue;
            }
            let key = i16::from(held.note.key) + i16::from(part.voicing.transpose);
            if let Ok(key) = u8::try_from(key) {
                down(KeyDown {
                    key,
                    color: part.color,
                    since: held.note.start,
                });
            }
        }
    }
}
