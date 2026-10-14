#![deny(missing_docs)]
#![no_std]
//! Second of three byte chunks of the pinned Noto Sans CJK SC variable font.
//!
//! The `cranpose-fonts` build script joins the chunks in order. Applications do not depend on
//! this crate directly.

/// Contiguous bytes from the pinned Noto Sans CJK SC variable font.
pub const DATA: &[u8] = include_bytes!("../data.bin");
