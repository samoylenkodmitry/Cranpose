//! A complete, bit-exact pure-Rust Basis Universal transcoder.
//!
//! Open a `.ktx2` or `.basis` Basis Universal texture and transcode any image
//! level to a GPU format. For every supported target the output matches the
//! canonical Basis Universal transcoder byte for byte.
//!
//! ```no_run
//! use basisu::{Transcoder, TargetFormat, DecodeFlags};
//! let bytes = std::fs::read("texture.ktx2").unwrap();
//! let tex = Transcoder::new(&bytes).unwrap();
//! let astc = tex.transcode(0, TargetFormat::Astc4x4Rgba, DecodeFlags::NONE).unwrap();
//! ```
//!
//! The [`Transcoder`] high-level API is the stable surface. The lower-level
//! codec modules are exposed (currently `pub`) for power users and the
//! conformance harness; they are not yet covered by semver stability.
//!
//! Without the default `std` feature the crate is `#![no_std]` (it still needs
//! `alloc`); the `zstd` feature implies `std`.

#![cfg_attr(not(feature = "std"), no_std)]

#[macro_use]
extern crate alloc;

mod api;
mod mathf;
#[doc(hidden)]
pub mod once;
mod support;
pub use api::{
    AstcBlock, DecodeFlags, Error, GlobalCodebook, ImageLevelInfo, SourceFormat, Supercompression,
    TargetFormat, Transcoder, VideoState,
};
pub use support::is_format_supported;

// The codec modules below back the stable `Transcoder` API. They are `pub` (but
// `#[doc(hidden)]`) so the conformance harness can drive individual codecs; they
// are not part of the stable, semver-covered surface.
#[doc(hidden)]
pub mod astc;
#[doc(hidden)]
pub mod basis;
#[doc(hidden)]
pub mod basislz;
#[doc(hidden)]
pub mod bitreader;
#[doc(hidden)]
pub mod color;
#[doc(hidden)]
pub mod dispatch;
#[doc(hidden)]
pub mod etc;
#[doc(hidden)]
pub mod fastenc;
#[doc(hidden)]
pub mod ktx2;
#[doc(hidden)]
pub mod pvrtc;
#[doc(hidden)]
pub mod tables;
#[doc(hidden)]
pub mod transcoder;
#[doc(hidden)]
pub mod uastc;
#[doc(hidden)]
pub mod uastc_hdr;
#[doc(hidden)]
pub mod uastc_hdr_6x6;
#[doc(hidden)]
pub mod xuastc;

/// View any slice of plain-old-data as its raw native-endian bytes, so a
/// generated table can be byte-compared against a packed expected table.
///
/// # Safety
///
/// `T` must have no padding bytes (every byte of every element initialized).
/// Padding is uninitialized memory, and viewing it through `&[u8]` is
/// undefined behavior. In practice this means `T` needs `#[repr(C)]` with a
/// field layout that leaves no gaps.
#[doc(hidden)]
pub unsafe fn pod_bytes<T: Copy>(s: &[T]) -> &[u8] {
    // SAFETY: the caller guarantees `T` is padding-free; length is exact.
    unsafe { core::slice::from_raw_parts(s.as_ptr() as *const u8, core::mem::size_of_val(s)) }
}
