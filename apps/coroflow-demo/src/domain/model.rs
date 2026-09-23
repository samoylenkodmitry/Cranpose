/// Identifies a note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NoteId(pub u64);

/// A note as the rest of the app sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// The note's identity.
    pub id: NoteId,
    /// What the user typed.
    pub title: String,
    /// Pinned notes sort first.
    pub pinned: bool,
}

/// The notes matching the current query, pinned first, newest first.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotesList {
    /// The query these notes were filtered with.
    pub query: String,
    /// The matching notes.
    pub visible: Vec<Note>,
    /// How many notes exist in total.
    pub total: usize,
}

/// One entry of the remote catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogHit {
    /// The entry's title.
    pub title: String,
    /// The topic it belongs to.
    pub topic: String,
}

/// The remote catalog search for the current query.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum CatalogResults {
    /// The query is too short to search.
    #[default]
    Idle,
    /// A request for `query` is in flight.
    Loading {
        /// The query being searched.
        query: String,
    },
    /// The catalog answered.
    Loaded {
        /// The query that was searched.
        query: String,
        /// Matching entries.
        hits: Vec<CatalogHit>,
    },
    /// The catalog failed.
    Failed {
        /// The query that was searched.
        query: String,
        /// Why it failed.
        message: String,
    },
}

/// Progress of the background sync.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SyncStatus {
    /// Nothing is being synced.
    #[default]
    Idle,
    /// Sync round `round` is running.
    Syncing {
        /// The running round.
        round: u32,
    },
    /// Sync round `round` finished.
    Synced {
        /// The last finished round.
        round: u32,
    },
}

/// Why a note operation failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NotesError {
    /// The title was blank.
    #[error("a note needs a title")]
    EmptyTitle,
    /// The title was longer than the limit.
    #[error("titles are limited to {0} characters")]
    TooLong(usize),
    /// No note has this id.
    #[error("that note no longer exists")]
    Missing(NoteId),
    /// The storage worker died.
    #[error("storage is unavailable")]
    Storage,
}

/// Why a catalog search failed.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CatalogError {
    /// The service rejected the request.
    #[error("the catalog service is unavailable")]
    Unavailable,
}
