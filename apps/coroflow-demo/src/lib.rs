//! Coroflow Notes: a Cranpose app layered the way large Android apps are.
//!
//! - [`data`] — a blocking store, an async remote catalog and an app-wide
//!   sync, exposed through repositories.
//! - [`domain`] — models, repository traits and use cases.
//! - [`presentation`] — view models exposing `StateFlow` screen state.
//! - [`ui`] — Cranpose screens; the only layer that imports Cranpose.
//! - [`di`] — the object graph.
//!
//! Only `ui` depends on Cranpose, so everything below it is tested on virtual
//! time with [`coroflow::TestScheduler`].

pub mod data;
pub mod di;
pub mod domain;
pub mod presentation;
pub mod ui;
