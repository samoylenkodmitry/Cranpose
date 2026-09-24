use coroflow::{FlowExt, MainScope, MutableStateFlow, SharingStarted, StateFlow};

use super::notes_view_model::{NotesUseCases, UI_STOP_TIMEOUT};
use crate::domain::model::{Note, NoteId};

/// The note screen's view model, one per visit of the screen.
pub struct NoteViewModel {
    scope: MainScope,
    note: StateFlow<Option<Note>>,
    deleted: MutableStateFlow<bool>,
    use_cases: NotesUseCases,
}

impl NoteViewModel {
    /// The view model of note `id`, whose coroutines run in `scope`.
    pub fn new(scope: MainScope, id: NoteId, use_cases: NotesUseCases) -> Self {
        let note = use_cases.observe_note.invoke(id).state_in(
            &scope,
            SharingStarted::while_subscribed(UI_STOP_TIMEOUT),
            None,
        );
        Self {
            scope,
            note,
            deleted: MutableStateFlow::new(false),
            use_cases,
        }
    }

    /// The note, or `None` before it loads and after it is deleted.
    pub fn note(&self) -> StateFlow<Option<Note>> {
        self.note.clone()
    }

    /// Whether the note has been deleted, which closes the screen.
    pub fn deleted(&self) -> StateFlow<bool> {
        self.deleted.as_state_flow()
    }

    /// The user tapped the pin.
    pub fn on_toggle_pinned(&self) {
        let (toggle, note) = (self.use_cases.toggle_pinned.clone(), self.note.value());
        if let Some(note) = note {
            self.scope.launch(async move {
                if let Err(error) = toggle.invoke(&note).await {
                    log::warn!("could not pin: {error}");
                }
            });
        }
    }

    /// The user deleted the note. [`deleted`](Self::deleted) turns true
    /// once it is gone, so the delete finishes before the screen closes.
    pub fn on_delete(&self) {
        let (delete, note) = (self.use_cases.delete_note.clone(), self.note.value());
        let deleted = self.deleted.clone();
        if let Some(note) = note {
            self.scope.launch(async move {
                match delete.invoke(note.id).await {
                    Ok(_) => deleted.set(true),
                    Err(error) => log::warn!("could not delete: {error}"),
                }
            });
        }
    }
}
