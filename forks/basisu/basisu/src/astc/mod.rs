//! Block-size-generic ASTC machinery, shared by every source codec whose
//! payload is (restricted or full) ASTC: the UASTC HDR 4x4 codec, the raw
//! ASTC LDR and ASTC HDR 6x6 sources, the UASTC HDR 6x6 intermediate codec,
//! and the XUASTC LDR codec.
//!
//! The pipeline runs in three stages: `unpack` a 16-byte physical block into a
//! logical block (BISE symbols and config), `dequant` its symbols ([0,255]
//! endpoints, [0,64] weights via `tables`), and `decode` texels in one of the
//! four decode modes (LDR8, SRGB8, HDR16, RGB9E5, the latter two via `half`
//! and `rgb9e5`). Everything here is footprint-generic (4x4 through 12x12);
//! the codec modules pass their block size.

pub mod decode;
pub mod dequant;
pub mod half;
pub(crate) mod hdr6x6;
pub mod pack;
pub mod rgb9e5;
pub mod slice;
pub mod tables;
pub mod unpack;
