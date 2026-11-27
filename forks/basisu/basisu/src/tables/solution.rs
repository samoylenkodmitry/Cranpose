//! The shared ETC1S-to-target "solution" table entry. The dxt1, bc7 mode-5
//! color, astc, atc, and pvrtc2 single-color match tables all use the same
//! `{ m_lo, m_hi, m_err }` entry shape, so a single type serves every table.

/// One single-color match candidate: the (lo, hi) endpoint pair and its error.
/// `#[repr(C)]` makes the entry exactly 4 bytes (`m_err` 2-byte aligned at
/// offset 2, no padding), giving tables a stable byte layout `as_bytes` can
/// expose.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Etc1ToSolution {
    pub m_lo: u8,
    pub m_hi: u8,
    pub m_err: u16,
}

/// View a solution table as its raw in-memory bytes (native endian). This is
/// safe because `Etc1ToSolution` is `#[repr(C)]`, has no padding, and every bit
/// pattern is a valid value.
pub fn as_bytes(table: &[Etc1ToSolution]) -> &[u8] {
    // SAFETY: contiguous `#[repr(C)]` POD, no padding; length is exact.
    unsafe {
        core::slice::from_raw_parts(table.as_ptr() as *const u8, core::mem::size_of_val(table))
    }
}
