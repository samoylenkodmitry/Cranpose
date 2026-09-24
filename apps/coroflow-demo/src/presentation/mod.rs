//! View models: screen state as `StateFlow`s and user intents as methods.
//! Depends on the domain and coroflow, never on Cranpose, so it is tested on
//! virtual time without a UI.

pub mod note_row_view_model;
pub mod note_view_model;
pub mod notes_messages;
pub mod notes_view_model;
