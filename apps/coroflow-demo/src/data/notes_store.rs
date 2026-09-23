use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use coroflow::{MutableStateFlow, StateFlow};

use crate::domain::model::{Note, NoteId, NotesError};

/// An in-memory table standing in for a database.
///
/// On native targets every write blocks for `write_latency`, the way a disk
/// write would, so the repository must call it from the I/O dispatcher. A
/// browser has no blocking storage, so there writes return at once.
pub struct NotesStore {
    rows: MutableStateFlow<Vec<Note>>,
    next_id: AtomicU64,
    write_latency: Duration,
}

impl NotesStore {
    /// A table holding `seed`, whose writes take `write_latency`.
    pub fn new(seed: Vec<Note>, write_latency: Duration) -> Self {
        let next_id = seed.iter().map(|note| note.id.0 + 1).max().unwrap_or(0);
        Self {
            rows: MutableStateFlow::new(seed),
            next_id: AtomicU64::new(next_id),
            write_latency,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn wait_for_disk(&self) {
        std::thread::sleep(self.write_latency);
    }

    #[cfg(target_arch = "wasm32")]
    fn wait_for_disk(&self) {
        let _ = self.write_latency;
    }

    /// The table's rows, updated after every write.
    pub fn observe(&self) -> StateFlow<Vec<Note>> {
        self.rows.as_state_flow()
    }

    /// Inserts a note, blocking for the write.
    pub fn insert(&self, title: String) -> Note {
        self.wait_for_disk();
        let note = Note {
            id: NoteId(self.next_id.fetch_add(1, Ordering::Relaxed)),
            title,
            pinned: false,
        };
        self.rows.update(|rows| {
            let mut rows = rows.clone();
            rows.push(note.clone());
            rows
        });
        note
    }

    /// Pins or unpins a note, blocking for the write.
    pub fn set_pinned(&self, id: NoteId, pinned: bool) -> Result<(), NotesError> {
        self.wait_for_disk();
        let mut found = false;
        self.rows.update(|rows| {
            found = rows.iter().any(|note| note.id == id);
            rows.iter()
                .map(|note| Note {
                    pinned: if note.id == id { pinned } else { note.pinned },
                    ..note.clone()
                })
                .collect()
        });
        if found {
            Ok(())
        } else {
            Err(NotesError::Missing(id))
        }
    }

    /// Deletes a note, blocking for the write.
    pub fn delete(&self, id: NoteId) -> Result<Note, NotesError> {
        self.wait_for_disk();
        let mut removed = None;
        self.rows.update(|rows| {
            let mut rows = rows.clone();
            removed = rows
                .iter()
                .position(|note| note.id == id)
                .map(|index| rows.remove(index));
            rows
        });
        removed.ok_or(NotesError::Missing(id))
    }
}

/// The notes a fresh install starts with.
pub fn starter_notes() -> Vec<Note> {
    [
        "Try typing \"flow\" in the search box",
        "Pin me to keep me on top",
        "Open Diagnostics to watch upstreams start and stop",
        "Search \"fail\" to see a catalog error",
    ]
    .into_iter()
    .enumerate()
    .map(|(index, title)| Note {
        id: NoteId(index as u64),
        title: title.to_string(),
        pinned: index == 0,
    })
    .collect()
}
