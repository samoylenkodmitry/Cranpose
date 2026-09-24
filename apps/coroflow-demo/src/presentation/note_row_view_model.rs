use std::rc::Rc;

use coroflow::{MainScope, MutableStateFlow, StateFlow};

use super::{
    notes_messages::{NotesEvent, NotesMessages},
    notes_view_model::NotesUseCases,
};
use crate::domain::model::Note;

/// The view model of one row in the notes list.
///
/// Each row asks for its own, keyed by the note's id, from the screen's view
/// model store. It survives the row scrolling out of view, so a pin or delete
/// still running when the row leaves finishes and reports through the
/// screen's [`NotesMessages`].
pub struct NoteRowViewModel {
    scope: MainScope,
    busy: MutableStateFlow<bool>,
    use_cases: NotesUseCases,
    messages: Rc<NotesMessages>,
}

impl NoteRowViewModel {
    /// A row view model whose coroutines run in `scope` and report to
    /// `messages`.
    pub fn new(scope: MainScope, use_cases: NotesUseCases, messages: Rc<NotesMessages>) -> Self {
        Self {
            scope,
            busy: MutableStateFlow::new(false),
            use_cases,
            messages,
        }
    }

    /// Whether a pin or delete for this row is still running.
    pub fn busy(&self) -> StateFlow<bool> {
        self.busy.as_state_flow()
    }

    /// The user tapped the row's pin.
    pub fn on_toggle_pinned(&self, note: Note) {
        let (toggle, busy) = (self.use_cases.toggle_pinned.clone(), self.busy.clone());
        busy.set(true);
        self.messages.report(&self.scope, async move {
            let result = toggle.invoke(&note).await;
            busy.set(false);
            result
                .err()
                .map(|error| NotesEvent::Failed(error.to_string()))
        });
    }

    /// The user deleted the row's note.
    pub fn on_delete(&self, note: Note) {
        let (delete, busy) = (self.use_cases.delete_note.clone(), self.busy.clone());
        busy.set(true);
        self.messages.report(&self.scope, async move {
            let result = delete.invoke(note.id).await;
            busy.set(false);
            Some(match result {
                Ok(note) => NotesEvent::Deleted(note.title),
                Err(error) => NotesEvent::Failed(error.to_string()),
            })
        });
    }
}
