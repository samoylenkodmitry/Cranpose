//! Where data comes from: a blocking local store, an asynchronous remote
//! catalog and an application-wide sync. Depends on the domain and coroflow,
//! never on the UI.

pub mod catalog_api;
pub mod notes_store;
pub mod repository;
pub mod sync;
