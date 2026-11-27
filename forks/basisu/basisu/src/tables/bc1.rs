//! BC1 single-color match tables.
//!
//! Four 256-entry tables map an 8-bit target channel value to the BC1 5-bit or
//! 6-bit endpoint pair that best reproduces it as a single color, one table per
//! (endpoint width, selector) combination. They are brute-forced on first use
//! and cached, rather than shipped as static data.

use crate::once::OnceBox;
use alloc::boxed::Box;
use alloc::vec::Vec;

/// One match-table entry: the winning (hi, lo) endpoint pair for one target
/// value. `#[repr(C)]` with two u8 fields pins each entry to two bytes in
/// `m_hi`, `m_lo` order, the same order `table_bytes` serializes.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Bc1MatchEntry {
    pub m_hi: u8,
    pub m_lo: u8,
}

/// The four BC1 single-color match tables.
pub struct Bc1MatchTables {
    /// 5-bit endpoints, selector 1, equal hi/lo allowed.
    pub match5_equals_1: [Bc1MatchEntry; 256],
    /// 6-bit endpoints, selector 1.
    pub match6_equals_1: [Bc1MatchEntry; 256],
    /// 5-bit endpoints, selector 0.
    pub match5_equals_0: [Bc1MatchEntry; 256],
    /// 6-bit endpoints, selector 0.
    pub match6_equals_0: [Bc1MatchEntry; 256],
}

/// Integer absolute value, as a helper to keep the error formulas terse.
#[inline]
fn iabs(x: i32) -> i32 {
    x.abs()
}

/// Fill one 256-entry match table: for each target value `i`, brute-force the
/// (lo, hi) endpoint pair over the `expand` decode table whose decoded single
/// color is closest under the selector model. `size0`/`size1` bound the lo/hi
/// search; the selector-0 tables pass `size0 == 1` to pin lo at 0.
fn prepare(table: &mut [Bc1MatchEntry; 256], expand: &[u8], size0: i32, size1: i32, sel: i32) {
    for i in 0..256i32 {
        let mut lowest_e = 256i32;
        for lo in 0..size0 {
            for hi in 0..size1 {
                let lo_e = expand[lo as usize] as i32;
                let hi_e = expand[hi as usize] as i32;
                let e = if sel == 1 {
                    // Selector 1: the 2:1 interpolated endpoint, plus a small
                    // tie-break penalty proportional to endpoint spread.
                    let mut e = iabs(((hi_e * 2 + lo_e) / 3) - i);
                    e += (iabs(hi_e - lo_e) * 3) / 100;
                    e
                } else {
                    debug_assert_eq!(sel, 0);
                    // Selector 0: the hi endpoint itself.
                    iabs(hi_e - i)
                };
                if e < lowest_e {
                    table[i as usize].m_hi = hi as u8;
                    table[i as usize].m_lo = lo as u8;
                    lowest_e = e;
                }
            }
        }
    }
}

/// Build all four tables from the standard BC1 5-bit and 6-bit component
/// expansions.
pub fn build() -> Bc1MatchTables {
    // bc1_expand5[i] = (i << 3) | (i >> 2); bc1_expand6[i] = (i << 2) | (i >> 4).
    let mut bc1_expand5 = [0u8; 32];
    for (i, e) in bc1_expand5.iter_mut().enumerate() {
        *e = ((i << 3) | (i >> 2)) as u8;
    }
    let mut bc1_expand6 = [0u8; 64];
    for (i, e) in bc1_expand6.iter_mut().enumerate() {
        *e = ((i << 2) | (i >> 4)) as u8;
    }

    let mut t = Bc1MatchTables {
        match5_equals_1: [Bc1MatchEntry::default(); 256],
        match6_equals_1: [Bc1MatchEntry::default(); 256],
        match5_equals_0: [Bc1MatchEntry::default(); 256],
        match6_equals_0: [Bc1MatchEntry::default(); 256],
    };
    prepare(&mut t.match5_equals_1, &bc1_expand5, 32, 32, 1);
    prepare(&mut t.match5_equals_0, &bc1_expand5, 1, 32, 0);
    prepare(&mut t.match6_equals_1, &bc1_expand6, 64, 64, 1);
    prepare(&mut t.match6_equals_0, &bc1_expand6, 1, 64, 0);
    t
}

/// Cached BC1 single-color match tables, built once on first use.
pub fn tables() -> &'static Bc1MatchTables {
    static T: OnceBox<Bc1MatchTables> = OnceBox::new();
    T.get_or_init(|| Box::new(build()))
}

impl Bc1MatchTables {
    /// The raw bytes of one table, two bytes per entry in field order (`m_hi`
    /// then `m_lo`).
    pub fn table_bytes(table: &[Bc1MatchEntry; 256]) -> Vec<u8> {
        let mut out = Vec::with_capacity(512);
        for e in table {
            out.push(e.m_hi);
            out.push(e.m_lo);
        }
        out
    }
}
