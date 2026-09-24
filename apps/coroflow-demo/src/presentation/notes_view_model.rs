use std::{rc::Rc, time::Duration};

use coroflow::{FlowExt, MainScope, MutableStateFlow, SharingStarted, StateFlow};

use super::notes_messages::{NotesEvent, NotesMessages};
use crate::domain::{
    model::{CatalogResults, NotesList, SyncStatus},
    use_cases::{
        AddNote, DeleteNote, ObserveNote, ObserveNotes, ObserveSync, SearchCatalog, TogglePinned,
    },
};

/// How long the screen state keeps its upstream after the screen stops
/// collecting it — long enough to survive a quick tab switch.
pub const UI_STOP_TIMEOUT: Duration = Duration::from_secs(5);

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

/// The use cases the notes screen needs, injected like a Hilt module would.
#[derive(Clone)]
pub struct NotesUseCases {
    /// Filters notes by the query.
    pub observe_notes: ObserveNotes,
    /// Follows one note.
    pub observe_note: ObserveNote,
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
    /// Whether the notes screen state is being computed, for the diagnostics
    /// screen.
    pub state_running: MutableStateFlow<bool>,
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
    use_cases: NotesUseCases,
    messages: Rc<NotesMessages>,
}

impl NotesViewModel {
    /// A view model whose coroutines run in `scope` and report to
    /// `messages`.
    pub fn new(scope: MainScope, use_cases: NotesUseCases, messages: Rc<NotesMessages>) -> Self {
        let query = MutableStateFlow::new(String::new());
        let (running_on_start, running_on_stop) = (
            use_cases.state_running.clone(),
            use_cases.state_running.clone(),
        );
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
            use_cases,
            messages,
        }
    }

    /// The screen state.
    pub fn ui_state(&self) -> StateFlow<NotesUiState> {
        self.ui_state.clone()
    }

    /// Whether the screen state is being computed right now.
    pub fn ui_upstream_running(&self) -> StateFlow<bool> {
        self.use_cases.state_running.as_state_flow()
    }

    /// The search query, so a screen coming back can show it again.
    pub fn query(&self) -> String {
        self.query.value()
    }

    /// The user edited the search box.
    pub fn on_query_changed(&self, query: String) {
        self.query.set(query);
    }

    /// The user asked to add a note titled `title`.
    pub fn on_add_note(&self, title: String) {
        let add = self.use_cases.add_note.clone();
        self.messages.report(&self.scope, async move {
            Some(match add.invoke(&title).await {
                Ok(note) => NotesEvent::Added(note.title),
                Err(error) => NotesEvent::Failed(error.to_string()),
            })
        });
    }
}
