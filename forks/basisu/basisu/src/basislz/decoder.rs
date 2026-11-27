//! The BasisLZ bit-stream reader (`bitwise_decoder`) and the symbol decoders
//! built on it: fixed-width `get_bits`, the variable-length `decode_vlc`,
//! `decode_rice` and `decode_truncated_binary`, and `decode_huffman`. The
//! `read_huffman_table` method loads an entropy-coded Huffman table from the
//! stream so `decode_huffman` has something to decode against.

use super::huffman::{HuffmanDecodingTable, FAST_LOOKUP_BITS};

// The entropy-coded Huffman-table format: the symbol-count cap and the special
// zero-run / repeat code points with their extra-bit widths.
const MAX_SYMS_LOG2: u32 = 14;
const MAX_SYMS: u32 = 1 << MAX_SYMS_LOG2;
const TOTAL_CODELENGTH_CODES: usize = 21;
const SMALL_ZERO_RUN_CODE: u32 = 17;
const BIG_ZERO_RUN_CODE: u32 = 18;
const SMALL_REPEAT_CODE: u32 = 19;
const SMALL_ZERO_RUN_SIZE_MIN: u32 = 3;
const SMALL_ZERO_RUN_EXTRA_BITS: u32 = 3;
const BIG_ZERO_RUN_SIZE_MIN: u32 = 11;
const BIG_ZERO_RUN_EXTRA_BITS: u32 = 7;
const SMALL_REPEAT_SIZE_MIN: u32 = 3;
const SMALL_REPEAT_EXTRA_BITS: u32 = 2;
const BIG_REPEAT_SIZE_MIN: u32 = 7;
const BIG_REPEAT_EXTRA_BITS: u32 = 7;

/// Transmission order of the code-length code sizes: the run/repeat specials
/// first, then the literal lengths from most to least common, so a table can
/// send only a short prefix and leave the rest at size zero.
const SORTED_CODELENGTH_CODES: [u8; TOTAL_CODELENGTH_CODES] = [
    17, 18, 19, 20, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15, 16,
];

/// Index of the highest set bit (floor of log2). Returns 0 for both 0 and 1.
fn floor_log2i(mut v: u32) -> u32 {
    let mut b = 0;
    while v > 1 {
        v >>= 1;
        b += 1;
    }
    b
}

/// The BasisLZ bit-stream decoder. Reads LSB-first; off the end reads zeros.
pub struct BitwiseDecoder<'a> {
    /// The source byte stream.
    buf: &'a [u8],
    /// Index of the next unread byte in `buf`.
    pos: usize,
    /// Refill buffer: up to 32 not-yet-consumed bits, oldest in the low bits.
    bit_buf: u32,
    /// Number of valid bits currently in `bit_buf`.
    bit_buf_size: u32,
}

impl<'a> BitwiseDecoder<'a> {
    /// Start a decoder at the front of `buf`.
    pub fn new(buf: &'a [u8]) -> Self {
        Self {
            buf,
            pos: 0,
            bit_buf: 0,
            bit_buf_size: 0,
        }
    }

    /// Next source byte, or 0 once the buffer is exhausted (the stream reads as
    /// zero-padded past the end).
    #[inline]
    fn next_byte(&mut self) -> u32 {
        if self.pos < self.buf.len() {
            let v = self.buf[self.pos] as u32;
            self.pos += 1;
            v
        } else {
            0
        }
    }

    /// Low `num_bits` of the stream without consuming them, refilling the bit
    /// buffer from `buf` as needed.
    pub fn peek_bits(&mut self, num_bits: u32) -> u32 {
        if num_bits == 0 {
            return 0;
        }
        while self.bit_buf_size < num_bits {
            let c = self.next_byte();
            self.bit_buf |= c << self.bit_buf_size;
            self.bit_buf_size += 8;
        }
        self.bit_buf & ((1u32 << num_bits) - 1)
    }

    /// Drop `num_bits` previously inspected with `peek_bits`.
    pub fn remove_bits(&mut self, num_bits: u32) {
        self.bit_buf >>= num_bits;
        self.bit_buf_size -= num_bits;
    }

    /// Read and consume `num_bits` (0..=32). The refill buffer is only 32 bits
    /// wide, so a request above 25 is split into a 25-bit read plus the rest.
    pub fn get_bits(&mut self, mut num_bits: u32) -> u32 {
        if num_bits > 25 {
            let bits0 = self.peek_bits(25);
            self.bit_buf >>= 25;
            self.bit_buf_size -= 25;
            num_bits -= 25;

            let bits = self.peek_bits(num_bits);
            self.bit_buf >>= num_bits;
            self.bit_buf_size -= num_bits;

            return bits0 | (bits << 25);
        }
        let bits = self.peek_bits(num_bits);
        self.bit_buf >>= num_bits;
        self.bit_buf_size -= num_bits;
        bits
    }

    /// Bits left to read before the stream turns into zero padding: unread
    /// source bytes plus whatever is still held in the refill buffer.
    pub fn bits_remaining(&self) -> usize {
        (self.buf.len() - self.pos) * 8 + self.bit_buf_size as usize
    }

    /// Truncated binary code over an alphabet of `n` symbols: the smaller values
    /// use `floor(log2 n)` bits, the rest one bit more.
    pub fn decode_truncated_binary(&mut self, n: u32) -> u32 {
        let k = floor_log2i(n);
        let u = (1u32 << (k + 1)) - n;
        let mut result = self.get_bits(k);
        if result >= u {
            result = ((result << 1) | self.get_bits(1)) - u;
        }
        result
    }

    /// Rice/Golomb decode with parameter `m`. The quotient is the run of
    /// leading 1 bits (counted 16 at a time); the result is `(quotient << m)`
    /// plus an `m`-bit remainder (read as `m + 1` bits with the low bit dropped).
    pub fn decode_rice(&mut self, m: u32) -> u32 {
        let mut q = 0u32;
        loop {
            let mut k = self.peek_bits(16);
            let mut l = 0u32;
            while k & 1 != 0 {
                l += 1;
                k >>= 1;
            }
            q += l;
            self.remove_bits(l);
            if l < 16 {
                break;
            }
        }
        (q << m) + (self.get_bits(m + 1) >> 1)
    }

    /// Variable-length code in `chunk_bits`-wide groups. Each group carries
    /// `chunk_bits` of payload plus one continuation bit; payload accumulates
    /// low end first and stops at a clear continuation bit (or once 32 bits are
    /// in hand).
    pub fn decode_vlc(&mut self, chunk_bits: u32) -> u32 {
        let chunk_size = 1u32 << chunk_bits;
        let chunk_mask = chunk_size - 1;
        let mut v = 0u32;
        let mut ofs = 0u32;
        loop {
            let s = self.get_bits(chunk_bits + 1);
            v |= (s & chunk_mask) << ofs;
            ofs += chunk_bits;
            if s & chunk_size == 0 {
                break;
            }
            if ofs >= 32 {
                break;
            }
        }
        v
    }

    /// Decode one Huffman symbol: a hit in the `FAST_LOOKUP_BITS`-wide direct
    /// table resolves in one step; a miss walks the overflow tree one bit at a
    /// time. Either way the symbol's code bits are consumed.
    pub fn decode_huffman(&mut self, ct: &HuffmanDecodingTable) -> u32 {
        let fast_lookup_size = 1u32 << FAST_LOOKUP_BITS;
        while self.bit_buf_size < 16 {
            let c = self.next_byte();
            self.bit_buf |= c << self.bit_buf_size;
            self.bit_buf_size += 8;
        }

        let mut sym = ct.lookup[(self.bit_buf & (fast_lookup_size - 1)) as usize];
        let code_len;
        if sym >= 0 {
            code_len = (sym >> 16) as u32;
            sym &= 0xFFFF;
        } else {
            let mut cl = FAST_LOOKUP_BITS;
            loop {
                let bit = (self.bit_buf >> cl) & 1;
                cl += 1;
                sym = ct.tree[(!sym + bit as i32) as usize] as i32;
                if sym >= 0 {
                    break;
                }
            }
            code_len = cl;
        }

        self.bit_buf >>= code_len;
        self.bit_buf_size -= code_len;
        sym as u32
    }

    /// Read an entropy-coded Huffman table from the stream into `ct`: the used
    /// symbol count, the code-length code sizes, then the run-length-coded
    /// per-symbol code sizes. Returns false on a malformed table.
    pub fn read_huffman_table(&mut self, ct: &mut HuffmanDecodingTable) -> bool {
        ct.clear();

        let total_used_syms = self.get_bits(MAX_SYMS_LOG2);
        if total_used_syms == 0 {
            return true;
        }
        if total_used_syms > MAX_SYMS {
            return false;
        }

        let mut code_length_code_sizes = [0u8; TOTAL_CODELENGTH_CODES];
        let num_codelength_codes = self.get_bits(5);
        if num_codelength_codes < 1 || num_codelength_codes as usize > TOTAL_CODELENGTH_CODES {
            return false;
        }
        for i in 0..num_codelength_codes as usize {
            code_length_code_sizes[SORTED_CODELENGTH_CODES[i] as usize] = self.get_bits(3) as u8;
        }

        let mut code_length_table = HuffmanDecodingTable::new();
        if !code_length_table.init(TOTAL_CODELENGTH_CODES, &code_length_code_sizes) {
            return false;
        }
        if !code_length_table.is_valid() {
            return false;
        }

        let total = total_used_syms as usize;
        let mut code_sizes = vec![0u8; total];
        let mut cur = 0usize;
        while cur < total {
            let c = self.decode_huffman(&code_length_table);
            if c <= 16 {
                code_sizes[cur] = c as u8;
                cur += 1;
            } else if c == SMALL_ZERO_RUN_CODE {
                cur +=
                    (self.get_bits(SMALL_ZERO_RUN_EXTRA_BITS) + SMALL_ZERO_RUN_SIZE_MIN) as usize;
            } else if c == BIG_ZERO_RUN_CODE {
                cur += (self.get_bits(BIG_ZERO_RUN_EXTRA_BITS) + BIG_ZERO_RUN_SIZE_MIN) as usize;
            } else {
                if cur == 0 {
                    return false;
                }
                let mut l = if c == SMALL_REPEAT_CODE {
                    self.get_bits(SMALL_REPEAT_EXTRA_BITS) + SMALL_REPEAT_SIZE_MIN
                } else {
                    self.get_bits(BIG_REPEAT_EXTRA_BITS) + BIG_REPEAT_SIZE_MIN
                };
                let prev = code_sizes[cur - 1];
                if prev == 0 {
                    return false;
                }
                loop {
                    if cur >= total {
                        return false;
                    }
                    code_sizes[cur] = prev;
                    cur += 1;
                    l -= 1;
                    if l == 0 {
                        break;
                    }
                }
            }
        }

        if cur != total {
            return false;
        }
        ct.init(total, &code_sizes)
    }
}
