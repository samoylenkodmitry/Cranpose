//! Decoder for canonical Huffman tables in the BasisLZ stream. Given the
//! per-symbol code sizes, it builds a flat fast-lookup array for short codes
//! plus an overflow binary tree for codes longer than [`FAST_LOOKUP_BITS`].

use alloc::vec::Vec;

/// Code-length cutoff for the flat fast-lookup path. Codes no longer than this
/// resolve in one array index; longer codes walk the overflow tree.
pub const FAST_LOOKUP_BITS: u32 = 10;
/// Size of the flat lookup array, one slot per `FAST_LOOKUP_BITS`-bit prefix.
const FAST_LOOKUP_SIZE: usize = 1 << FAST_LOOKUP_BITS;
/// Largest code size the decoder accepts. A longer code size means the table is
/// malformed and [`HuffmanDecodingTable::init`] rejects it.
const MAX_INTERNAL_CODE_SIZE: usize = 31;

/// A built Huffman decoding table.
///
/// `lookup` is indexed by the low `FAST_LOOKUP_BITS` of the (bit-reversed)
/// incoming bits. A non-negative slot packs `code_size << 16 | symbol`; a
/// negative slot is an index into `tree`, the overflow binary tree for long
/// codes. `tree` nodes are likewise negative for interior links and hold the
/// symbol index at leaves.
#[derive(Clone, Default)]
pub struct HuffmanDecodingTable {
    /// Per-symbol code length, 0 for symbols absent from the code.
    pub code_sizes: Vec<u8>,
    /// Flat lookup for short codes, indexed by bit-reversed prefix.
    pub lookup: Vec<i32>,
    /// Overflow binary tree for codes longer than `FAST_LOOKUP_BITS`.
    pub tree: Vec<i16>,
}

impl HuffmanDecodingTable {
    /// An empty table. Not usable until [`init`](Self::init) succeeds.
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop the built table, returning to the empty state.
    pub fn clear(&mut self) {
        self.code_sizes.clear();
        self.lookup.clear();
        self.tree.clear();
    }

    /// True once a table has been built (it has at least one code size).
    pub fn is_valid(&self) -> bool {
        !self.code_sizes.is_empty()
    }

    /// Build the lookup and tree from the canonical code sizes of `total_syms`
    /// symbols. Returns false if the sizes do not form a valid prefix code
    /// (over-long size, slot collision, or a count that is neither complete nor
    /// the single-symbol special case).
    pub fn init(&mut self, total_syms: usize, code_sizes: &[u8]) -> bool {
        self.clear();
        if total_syms == 0 {
            return true;
        }

        self.code_sizes = code_sizes[..total_syms].to_vec();
        self.lookup = vec![0i32; FAST_LOOKUP_SIZE];
        self.tree = vec![0i16; total_syms * 2];

        // Histogram of code sizes, rejecting any that exceed the supported max.
        let mut syms_using_codesize = [0u32; MAX_INTERNAL_CODE_SIZE + 1];
        for &cs in &code_sizes[..total_syms] {
            if cs as usize > MAX_INTERNAL_CODE_SIZE {
                return false;
            }
            syms_using_codesize[cs as usize] += 1;
        }

        // First canonical code value for each length. The running `total` is the
        // next code shifted left by one per length step, which is the standard
        // canonical-Huffman assignment.
        let mut next_code = [0u32; MAX_INTERNAL_CODE_SIZE + 2];
        let mut used_syms = 0u32;
        let mut total = 0u32;
        for i in 1..MAX_INTERNAL_CODE_SIZE {
            used_syms += syms_using_codesize[i];
            total = total.wrapping_add(syms_using_codesize[i]) << 1;
            next_code[i + 1] = total;
        }

        // The code must be complete (fill the whole 2^max code space). The one
        // exception is a single used symbol: its lone short code cannot fill
        // the space, so that case is allowed through.
        if ((1u32 << MAX_INTERNAL_CODE_SIZE) != total) && (used_syms != 1) {
            return false;
        }

        // Interior tree nodes are allocated as the odd negative indices -1, -3,
        // -5, ... `tree_next` hands out the next one.
        let mut tree_next: i32 = -1;
        for (sym_index, &cs) in code_sizes[..total_syms].iter().enumerate() {
            let code_size = cs as u32;
            if code_size == 0 {
                continue;
            }
            let mut cur_code = next_code[code_size as usize];
            next_code[code_size as usize] += 1;

            // The stream feeds bits LSB-first, so index by the reversed code.
            let mut rev_code = 0u32;
            for _ in 0..code_size {
                rev_code = (rev_code << 1) | (cur_code & 1);
                cur_code >>= 1;
            }

            if code_size <= FAST_LOOKUP_BITS {
                // Short code: fill every fast-lookup slot whose low bits match
                // this code, stepping by 2^code_size to cover the don't-care
                // high bits. A non-zero slot means two codes collided.
                let k = ((code_size << 16) | sym_index as u32) as i32;
                let mut rc = rev_code;
                while (rc as usize) < FAST_LOOKUP_SIZE {
                    if self.lookup[rc as usize] != 0 {
                        return false;
                    }
                    self.lookup[rc as usize] = k;
                    rc += 1 << code_size;
                }
                continue;
            }

            // Long code: its first FAST_LOOKUP_BITS bits select a lookup slot
            // that roots an overflow subtree. Allocate the root node if absent;
            // a non-negative slot here means a short code already claimed it.
            let idx0 = (rev_code & (FAST_LOOKUP_SIZE as u32 - 1)) as usize;
            let mut tree_cur = self.lookup[idx0];
            if tree_cur == 0 {
                self.lookup[idx0] = tree_next;
                tree_cur = tree_next;
                tree_next -= 2;
            }
            if tree_cur >= 0 {
                return false;
            }

            // Walk the remaining bits past the lookup prefix. An interior node
            // id -n owns the adjacent slot pair tree[n - 1] (bit 0) and tree[n]
            // (bit 1); child nodes are allocated on demand down to the last bit.
            let mut rc = rev_code >> (FAST_LOOKUP_BITS - 1);

            let mut j = code_size as i32;
            while j > FAST_LOOKUP_BITS as i32 + 1 {
                rc >>= 1;
                tree_cur -= (rc & 1) as i32;
                let idx = -tree_cur - 1;
                if idx < 0 {
                    return false;
                }
                let idx = idx as usize;
                if idx >= self.tree.len() {
                    self.tree.resize(idx + 1, 0);
                }
                if self.tree[idx] == 0 {
                    self.tree[idx] = tree_next as i16;
                    tree_cur = tree_next;
                    tree_next -= 2;
                } else {
                    tree_cur = self.tree[idx] as i32;
                    if tree_cur >= 0 {
                        return false;
                    }
                }
                j -= 1;
            }

            // Final bit reaches a leaf, where the symbol index is stored. A
            // non-zero leaf means two codes mapped to the same path.
            rc >>= 1;
            tree_cur -= (rc & 1) as i32;
            let idx = -tree_cur - 1;
            if idx < 0 {
                return false;
            }
            let idx = idx as usize;
            if idx >= self.tree.len() {
                self.tree.resize(idx + 1, 0);
            }
            if self.tree[idx] != 0 {
                return false;
            }
            self.tree[idx] = sym_index as i16;
        }

        true
    }
}
