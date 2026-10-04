#![no_std]

/// Contiguous bytes from the pinned Noto Sans CJK SC variable font.
pub const DATA: &[u8] = include_bytes!("../data.bin");
