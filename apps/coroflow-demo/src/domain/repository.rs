use coroflow::{BoxFlow, BoxFuture, StateFlow};

use super::model::{CatalogResults, Note, NoteId, NotesError, SyncStatus};

/// Everything the domain needs from note storage and the remote catalog.
///
/// Flows are cold until collected; futures are Kotlin `suspend` functions.
/// Implementations are main-safe: they move blocking work off the caller's
/// thread themselves.
pub trait NotesRepository: Send + Sync {
    /// All notes, re-emitted after every change.
    fn observe_notes(&self) -> BoxFlow<Vec<Note>>;

    /// Searches the remote catalog, emitting `Loading` and then the outcome.
    fn search_catalog(&self, query: String) -> BoxFlow<CatalogResults>;

    /// Stores a new note.
    fn add_note(&self, title: String) -> BoxFuture<Result<Note, NotesError>>;

    /// Pins or unpins a note.
    fn set_pinned(&self, id: NoteId, pinned: bool) -> BoxFuture<Result<(), NotesError>>;

    /// Deletes a note and returns it.
    fn delete_note(&self, id: NoteId) -> BoxFuture<Result<Note, NotesError>>;
}

/// The background sync, as the domain sees it.
pub trait SyncRepository: Send + Sync {
    /// The sync progress; the sync runs only while someone collects this.
    fn status(&self) -> StateFlow<SyncStatus>;
}
