// A piece of music as the notes of its parts, timed in seconds at the tempo
// the piece was written with.

use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    // Seconds from the start of the song to the press of the key.
    pub start: f32,
    // Seconds the key stays down.
    pub held: f32,
    // Seconds the string sounds: `held`, or longer while the pedal keeps
    // the damper up.
    pub ring: f32,
    // The MIDI number of the key; 60 is middle C.
    pub key: u8,
    // How hard the key is struck, from 1 to 127.
    pub velocity: u8,
}

// Song seconds as a note keeps them: a piece of an hour still tells its
// notes apart by a quarter of a millisecond.
pub(super) fn note_seconds(seconds: f64) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the times of a song fit an f32 closer than a frame of sound"
    )]
    let seconds = seconds as f32;
    seconds
}

impl Note {
    pub fn key_released_at(&self) -> f32 {
        self.start + self.held
    }

    pub fn silent_at(&self) -> f32 {
        self.start + self.ring
    }
}

// The notes of one part, ordered by `start`. Clones share the notes.
#[derive(Clone, Debug)]
pub struct Notes {
    notes: Arc<[Note]>,
    seconds: f32,
    longest_held: f32,
    longest_ring: f32,
}

impl Notes {
    // Orders `notes` by their start.
    pub fn new(mut notes: Vec<Note>) -> Self {
        notes.sort_by(|a, b| a.start.total_cmp(&b.start));
        Self {
            seconds: notes.iter().map(Note::silent_at).fold(0.0, f32::max),
            longest_held: notes.iter().map(|note| note.held).fold(0.0, f32::max),
            longest_ring: notes.iter().map(|note| note.ring).fold(0.0, f32::max),
            notes: notes.into(),
        }
    }

    pub fn as_slice(&self) -> &[Note] {
        &self.notes
    }

    // The index of the first note that starts at `seconds` or later.
    pub fn first_at_or_after(&self, seconds: f64) -> usize {
        self.notes
            .partition_point(|note| f64::from(note.start) < seconds)
    }

    // Seconds from the start of the song to the end of the last sound.
    pub fn seconds(&self) -> f32 {
        self.seconds
    }

    // The longest time a key of this part stays down, in seconds.
    pub fn longest_held(&self) -> f32 {
        self.longest_held
    }

    // The longest time a note of this part sounds, in seconds.
    pub fn longest_ring(&self) -> f32 {
        self.longest_ring
    }

    pub(super) fn same_notes(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.notes, &other.notes)
    }
}

impl PartialEq for Notes {
    fn eq(&self, other: &Self) -> bool {
        self.same_notes(other)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SongPart {
    pub name: String,
    pub notes: Notes,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Song {
    pub parts: Vec<SongPart>,
}

impl Song {
    // Seconds from the start of the song to the end of its last sound.
    pub fn seconds(&self) -> f32 {
        self.parts
            .iter()
            .map(|part| part.notes.seconds())
            .fold(0.0, f32::max)
    }
}
