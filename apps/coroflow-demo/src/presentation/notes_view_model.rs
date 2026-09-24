use std::{future::Future, time::Duration};

use coroflow::{
    FlowExt, MainScope, MutableSharedFlow, MutableStateFlow, SharedFlow, SharingStarted, StateFlow,
};

use crate::domain::{
    model::{CatalogResults, Note, NoteId, NotesList, SyncStatus},
    use_cases::{AddNote, DeleteNote, ObserveNotes, ObserveSync, SearchCatalog, TogglePinned},
};

/// How long the screen state keeps its upstream after the screen stops
/// collecting it — long enough to survive a quick tab switch.
pub const UI_STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// How many undelivered events the view model keeps for a slow screen.
pub const EVENT_BUFFER: usize = 16;

/// Everything the notes screen draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NotesUiState {
    /// The notes matching the query.
    pub notes: NotesList,
    /// The remote catalog search for the query.
    pub catalog: CatalogResults,
    /// The background sync.
    pub sync: SyncStatus,
}

/// One-shot messages for the screen, such as a snackbar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotesEvent {
    /// A note was added.
    Added(String),
    /// A note was deleted.
    Deleted(String),
    /// An action failed.
    Failed(String),
}

impl NotesEvent {
    /// The text to show the user.
    pub fn message(&self) -> String {
        match self {
            NotesEvent::Added(title) => format!("Added \u{201c}{title}\u{201d}"),
            NotesEvent::Deleted(title) => format!("Deleted \u{201c}{title}\u{201d}"),
            NotesEvent::Failed(reason) => format!("Failed: {reason}"),
        }
    }
}

/// The use cases the notes screen needs, injected like a Hilt module would.
#[derive(Clone)]
pub struct NotesUseCases {
    /// Filters notes by the query.
    pub observe_notes: ObserveNotes,
    /// Searches the remote catalog.
    pub search_catalog: SearchCatalog,
    /// Adds a note.
    pub add_note: AddNote,
    /// Pins and unpins notes.
    pub toggle_pinned: TogglePinned,
    /// Deletes notes.
    pub delete_note: DeleteNote,
    /// Observes the background sync.
    pub observe_sync: ObserveSync,
}

/// The notes screen's view model.
///
/// It lives on the main thread in its [`MainScope`] and knows nothing about
/// Cranpose: the screen reads [`ui_state`](Self::ui_state) and
/// [`events`](Self::events) and calls the `on_*` methods.
pub struct NotesViewModel {
    scope: MainScope,
    query: MutableStateFlow<String>,
    ui_state: StateFlow<NotesUiState>,
    events: MutableSharedFlow<NotesEvent>,
    ui_upstream_running: MutableStateFlow<bool>,
    use_cases: NotesUseCases,
}

impl NotesViewModel {
    /// A view model whose coroutines run in `scope`.
    pub fn new(scope: MainScope, use_cases: NotesUseCases) -> Self {
        let query = MutableStateFlow::new(String::new());
        let ui_upstream_running = MutableStateFlow::new(false);
        let (running_on_start, running_on_stop) =
            (ui_upstream_running.clone(), ui_upstream_running.clone());
        let ui_state = use_cases
            .observe_notes
            .invoke(query.as_state_flow())
            .combine(
                use_cases.search_catalog.invoke(query.as_state_flow()),
                |notes, catalog| (notes.clone(), catalog.clone()),
            )
            .combine(use_cases.observe_sync.invoke(), |(notes, catalog), sync| {
                NotesUiState {
                    notes: notes.clone(),
                    catalog: catalog.clone(),
                    sync: *sync,
                }
            })
            .on_start(move || running_on_start.set(true))
            .on_completion(move || running_on_stop.set(false))
            .state_in(
                &scope,
                SharingStarted::while_subscribed(UI_STOP_TIMEOUT),
                NotesUiState::default(),
            );
        Self {
            scope,
            query,
            ui_state,
            events: MutableSharedFlow::new(0, EVENT_BUFFER),
            ui_upstream_running,
            use_cases,
        }
    }

    /// The screen state.
    pub fn ui_state(&self) -> StateFlow<NotesUiState> {
        self.ui_state.clone()
    }

    /// One-shot messages for the screen.
    pub fn events(&self) -> SharedFlow<NotesEvent> {
        self.events.as_shared_flow()
    }

    /// Whether the screen state is being computed right now.
    pub fn ui_upstream_running(&self) -> StateFlow<bool> {
        self.ui_upstream_running.as_state_flow()
    }

    /// The user edited the search box.
    pub fn on_query_changed(&self, query: String) {
        self.query.set(query);
    }

    /// The user asked to add a note titled `title`.
    pub fn on_add_note(&self, title: String) {
        let add = self.use_cases.add_note.clone();
        self.launch_reporting(async move {
            Some(match add.invoke(&title).await {
                Ok(note) => NotesEvent::Added(note.title),
                Err(error) => NotesEvent::Failed(error.to_string()),
            })
        });
    }

    /// The user tapped a note's pin.
    pub fn on_toggle_pinned(&self, note: Note) {
        let toggle = self.use_cases.toggle_pinned.clone();
        self.launch_reporting(async move {
            toggle
                .invoke(&note)
                .await
                .err()
                .map(|error| NotesEvent::Failed(error.to_string()))
        });
    }

    /// The user deleted a note.
    pub fn on_delete(&self, id: NoteId) {
        let delete = self.use_cases.delete_note.clone();
        self.launch_reporting(async move {
            Some(match delete.invoke(id).await {
                Ok(note) => NotesEvent::Deleted(note.title),
                Err(error) => NotesEvent::Failed(error.to_string()),
            })
        });
    }

    fn launch_reporting(&self, action: impl Future<Output = Option<NotesEvent>> + 'static) {
        let events = self.events.clone();
        self.scope.launch(async move {
            if let Some(event) = action.await {
                events.emit(event).await;
            }
        });
    }
}
