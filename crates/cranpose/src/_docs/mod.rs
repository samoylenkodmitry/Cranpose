//! Guides for using Cranpose.
//!
//! These pages live in the crate rather than in a separate site so they are
//! version-locked to the code and their examples are compiled by
//! `cargo test --doc`. A guide that stops compiling is a guide that is wrong,
//! and CI finds out before a reader does.
//!
//! - [`getting_started`] -- a window on screen.
//! - [`state`] -- state that survives recomposition.
//! - [`layout`] -- modifiers, stacks, and lists that stay cheap.
//! - [`platforms`] -- what each target needs.

pub mod getting_started;
pub mod layout;
pub mod platforms;
pub mod state;
