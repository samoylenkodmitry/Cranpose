use std::future::Future;

use coroflow::{MainScope, MutableSharedFlow, SharedFlow};

/// How many undelivered messages the bus keeps for a slow screen.
pub const MESSAGE_BUFFER: usize = 16;

/// One-shot messages, such as a snackbar, for the notes screen.
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

/// The notes screen's message bus: the screen's view model and every row's
/// view model report through it, and the screen shows what arrives.
///
/// It lives in the screen's view model store next to them, so they all get
/// the same bus without knowing about each other — the arch starter's
/// `ScreenBus`.
pub struct NotesMessages {
    events: MutableSharedFlow<NotesEvent>,
}

impl Default for NotesMessages {
    fn default() -> Self {
        Self {
            events: MutableSharedFlow::new(0, MESSAGE_BUFFER),
        }
    }
}

impl NotesMessages {
    /// The messages, as they are reported.
    pub fn events(&self) -> SharedFlow<NotesEvent> {
        self.events.as_shared_flow()
    }

    /// Runs `action` in `scope` and reports the message it ends with, if any.
    pub fn report(
        &self,
        scope: &MainScope,
        action: impl Future<Output = Option<NotesEvent>> + 'static,
    ) {
        let events = self.events.clone();
        scope.launch(async move {
            if let Some(event) = action.await {
                events.emit(event).await;
            }
        });
    }
}
