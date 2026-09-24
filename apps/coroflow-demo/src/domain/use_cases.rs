use std::{sync::Arc, time::Duration};

use coroflow::{Dispatcher, FlowExt, SendFlow, StateFlow};

use super::{
    model::{CatalogResults, Note, NoteId, NotesError, NotesList, SyncStatus},
    repository::{NotesRepository, SyncRepository},
};

/// How long the query must stay unchanged before the catalog is searched.
pub const CATALOG_SEARCH_DEBOUNCE: Duration = Duration::from_millis(400);

/// The shortest query sent to the catalog.
pub const MIN_CATALOG_QUERY: usize = 2;

/// The longest title a note may have.
pub const MAX_TITLE_CHARS: usize = 80;

/// Filters the notes by a query, off the main thread.
#[derive(Clone)]
pub struct ObserveNotes {
    repository: Arc<dyn NotesRepository>,
    compute: Dispatcher,
}

impl ObserveNotes {
    /// Filters on `compute`.
    pub fn new(repository: Arc<dyn NotesRepository>, compute: Dispatcher) -> Self {
        Self {
            repository,
            compute,
        }
    }

    /// The notes matching each query, updated when either changes.
    pub fn invoke(&self, query: impl SendFlow<Item = String>) -> impl SendFlow<Item = NotesList> {
        query
            .combine(self.repository.observe_notes(), |query, notes| {
                filter_notes(query, notes)
            })
            .flow_on(self.compute.clone())
    }
}

/// Follows one note.
#[derive(Clone)]
pub struct ObserveNote {
    repository: Arc<dyn NotesRepository>,
}

impl ObserveNote {
    /// Reads through `repository`.
    pub fn new(repository: Arc<dyn NotesRepository>) -> Self {
        Self { repository }
    }

    /// The note `id` after every change to it, and `None` once it is deleted.
    pub fn invoke(&self, id: NoteId) -> impl SendFlow<Item = Option<Note>> {
        self.repository
            .observe_notes()
            .map(move |notes| notes.into_iter().find(|note| note.id == id))
            .distinct_until_changed()
    }
}

/// Selects the notes whose title contains `query`, pinned first, newest first.
pub fn filter_notes(query: &str, notes: &[Note]) -> NotesList {
    let needle = query.trim().to_lowercase();
    let mut visible: Vec<Note> = notes
        .iter()
        .filter(|note| needle.is_empty() || note.title.to_lowercase().contains(&needle))
        .cloned()
        .collect();
    visible.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.id.0.cmp(&a.id.0)));
    NotesList {
        query: query.to_string(),
        visible,
        total: notes.len(),
    }
}

/// Searches the remote catalog as the user types.
#[derive(Clone)]
pub struct SearchCatalog {
    repository: Arc<dyn NotesRepository>,
}

impl SearchCatalog {
    /// Searches through `repository`.
    pub fn new(repository: Arc<dyn NotesRepository>) -> Self {
        Self { repository }
    }

    /// Debounces the query and searches the newest one, cancelling the request
    /// for any query the user has already typed past.
    pub fn invoke(
        &self,
        query: impl SendFlow<Item = String>,
    ) -> impl SendFlow<Item = CatalogResults> {
        let repository = Arc::clone(&self.repository);
        query
            .map(|query| query.trim().to_string())
            .debounce(CATALOG_SEARCH_DEBOUNCE)
            .distinct_until_changed()
            .transform_latest(async move |query, emitter| {
                if query.chars().count() < MIN_CATALOG_QUERY {
                    emitter.emit(CatalogResults::Idle).await;
                } else {
                    emitter.emit_all(repository.search_catalog(query)).await;
                }
            })
            .start_with(CatalogResults::Idle)
    }
}

/// Validates and stores a new note.
#[derive(Clone)]
pub struct AddNote {
    repository: Arc<dyn NotesRepository>,
}

impl AddNote {
    /// Stores through `repository`.
    pub fn new(repository: Arc<dyn NotesRepository>) -> Self {
        Self { repository }
    }

    /// Stores a note titled `title` once it passes validation.
    pub async fn invoke(&self, title: &str) -> Result<Note, NotesError> {
        let title = title.trim();
        if title.is_empty() {
            return Err(NotesError::EmptyTitle);
        }
        if title.chars().count() > MAX_TITLE_CHARS {
            return Err(NotesError::TooLong(MAX_TITLE_CHARS));
        }
        self.repository.add_note(title.to_string()).await
    }
}

/// Flips a note between pinned and unpinned.
#[derive(Clone)]
pub struct TogglePinned {
    repository: Arc<dyn NotesRepository>,
}

impl TogglePinned {
    /// Stores through `repository`.
    pub fn new(repository: Arc<dyn NotesRepository>) -> Self {
        Self { repository }
    }

    /// Pins `note` if it is unpinned, and unpins it otherwise.
    pub async fn invoke(&self, note: &Note) -> Result<(), NotesError> {
        self.repository.set_pinned(note.id, !note.pinned).await
    }
}

/// Deletes a note.
#[derive(Clone)]
pub struct DeleteNote {
    repository: Arc<dyn NotesRepository>,
}

impl DeleteNote {
    /// Deletes through `repository`.
    pub fn new(repository: Arc<dyn NotesRepository>) -> Self {
        Self { repository }
    }

    /// Deletes the note `id` and returns it.
    pub async fn invoke(&self, id: NoteId) -> Result<Note, NotesError> {
        self.repository.delete_note(id).await
    }
}

/// Observes the background sync.
#[derive(Clone)]
pub struct ObserveSync {
    repository: Arc<dyn SyncRepository>,
}

impl ObserveSync {
    /// Observes `repository`.
    pub fn new(repository: Arc<dyn SyncRepository>) -> Self {
        Self { repository }
    }

    /// The sync progress.
    pub fn invoke(&self) -> StateFlow<SyncStatus> {
        self.repository.status()
    }
}
