//! UASTC transcoding: unpack a 128-bit UASTC block into structured form and
//! into pixels, then transcode to target formats.
//!
//! The pieces stack as mode property tables (`tables`), partition and BISE
//! tables, block unpack (`unpack`), then the per-target transcoders. This
//! module holds the mode-count and solid-color-mode constants shared across
//! them.

pub mod astc_pack;
pub mod astc_pack_tables;
pub mod bc1;
pub mod bc3_bc5;
pub mod bc4;
pub mod bc7;
pub mod bc7_tables;
pub mod bise;
pub mod eac;
pub mod huff_modes;
pub mod partitions;
pub mod patterns;
pub mod tables;
pub mod unpack;

/// Total UASTC block modes.
pub const TOTAL_UASTC_MODES: usize = 19;
/// The mode index reserved for solid-color blocks.
pub const UASTC_MODE_INDEX_SOLID_COLOR: u32 = 8;

#[cfg(test)]
mod tests {
    #[test]
    fn no_padding_in_flat_uastc_tables() {
        // The table conformance tests compare flat tables as raw bytes, so
        // the element types must be padding-free.
        assert_eq!(core::mem::size_of::<[u32; 2]>(), 8);
        assert_eq!(core::mem::size_of::<[i32; 3]>(), 12);
        assert_eq!(core::mem::size_of::<[u8; 3]>(), 3);
    }
}
