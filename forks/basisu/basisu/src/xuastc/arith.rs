//! The XUASTC arithmetic (range) decoder and its adaptive models: a
//! carry-less range coder with byte-wise renormalization, plus adaptive
//! binary, gamma, and multi-symbol models whose probability snapshots update
//! on decaying intervals. Every model update is integer arithmetic (the
//! `0x80000000 / count` scale divisions and the fixed shift constants). Each
//! decoded symbol feeds the next model state, so the state has to stay
//! bit-exact across the whole stream; floating point would let rounding drift.

use alloc::vec;
use alloc::vec::Vec;

const DM_LEN_SHIFT: u32 = 15;
const DM_MAX_COUNT: u32 = 1 << DM_LEN_SHIFT; // 32768
const BM_LEN_SHIFT: u32 = 13;
const BM_MAX_COUNT: u32 = 1 << BM_LEN_SHIFT; // 8192
const ARITH_MIN_LEN: u32 = 1 << 24;
/// Minimum decoder input in bytes. `new` preloads four bytes into `value`, so
/// a shorter buffer cannot start the range coder and is rejected.
pub const ARITH_MIN_EXPECTED_DATA_BUF_SIZE: usize = 5;

/// Adaptive binary (two-symbol) probability model.
#[derive(Clone)]
pub struct BitModel {
    bit0_prob: u32,
    bit0_count: u32,
    bit_count: u32,
    bits_until_update: i32,
    update_interval: u32,
}

impl Default for BitModel {
    fn default() -> Self {
        Self {
            bit0_count: 1,
            bit_count: 2,
            bit0_prob: 1 << (BM_LEN_SHIFT - 1),
            update_interval: 4,
            bits_until_update: 4,
        }
    }
}

impl BitModel {
    /// Recompute the bit-0 probability from the running counts, halving both
    /// when they saturate, then stretch the interval before the next update.
    fn update(&mut self) {
        if self.bit_count >= BM_MAX_COUNT {
            self.bit_count = (self.bit_count + 1) >> 1;
            self.bit0_count = (self.bit0_count + 1) >> 1;
            if self.bit0_count == self.bit_count {
                self.bit_count += 1;
            }
        }
        let scale = 0x8000_0000u32 / self.bit_count;
        self.bit0_prob = (self.bit0_count * scale) >> (31 - BM_LEN_SHIFT);
        self.update_interval = ((5 * self.update_interval) >> 2).clamp(4, 128);
        self.bits_until_update = self.update_interval as i32;
    }
}

/// Bit models for the adaptive gamma code: one set for the unary prefix, one
/// for the binary tail.
#[derive(Clone, Default)]
pub struct GammaContexts {
    prefix: [BitModel; 3],
    tail: [BitModel; 4],
}

/// Adaptive multi-symbol model: a live symbol-frequency histogram plus a
/// snapshot cumulative-frequency table that the decoder binary-searches.
#[derive(Clone, Default)]
pub struct DataModel {
    num_data_syms: u32,
    sym_freqs: Vec<u32>,
    total_sym_freq: u32,
    cum_sym_freqs: Vec<u32>,
    update_interval: u32,
    num_syms_until_next_update: i32,
}

impl DataModel {
    /// Build a model over `num_syms` symbols. `faster_update` shortens the
    /// initial update interval so the model adapts sooner.
    pub fn new(num_syms: u32, faster_update: bool) -> Self {
        let mut m = Self::default();
        m.init(num_syms, faster_update);
        m
    }

    /// Reset the model: every symbol starts at frequency one, then take an
    /// immediate cumulative-frequency snapshot.
    pub fn init(&mut self, num_syms: u32, faster_update: bool) {
        debug_assert!((2..=2048).contains(&num_syms));
        self.num_data_syms = num_syms;
        self.sym_freqs = vec![1; num_syms as usize];
        self.cum_sym_freqs = vec![0; num_syms as usize + 1];
        self.total_sym_freq = num_syms;
        self.update_interval = num_syms;
        self.num_syms_until_next_update = 0;
        self.update();
        if faster_update {
            self.update_interval = num_syms.div_ceil(8).clamp(4, (num_syms + 6) << 3);
            self.num_syms_until_next_update = self.update_interval as i32;
        }
    }

    /// True once `init` ran (the 5-D submode models initialize lazily).
    pub fn is_initialized(&self) -> bool {
        self.num_data_syms != 0
    }

    /// Rebuild the cumulative-frequency snapshot from the live histogram,
    /// halving every frequency when their total saturates, then stretch the
    /// update interval.
    fn update(&mut self) {
        let n = self.num_data_syms as usize;
        while self.total_sym_freq >= DM_MAX_COUNT {
            self.total_sym_freq = 0;
            for f in self.sym_freqs.iter_mut() {
                *f = (*f + 1) >> 1;
                self.total_sym_freq += *f;
            }
        }
        let scale = 0x8000_0000u32 / self.total_sym_freq;
        let mut sum = 0u32;
        for i in 0..n {
            self.cum_sym_freqs[i] = (scale * sum) >> (31 - DM_LEN_SHIFT);
            sum += self.sym_freqs[i];
        }
        self.cum_sym_freqs[n] = DM_MAX_COUNT;
        self.update_interval =
            ((5 * self.update_interval) >> 2).clamp(4, (self.num_data_syms + 6) << 3);
        self.num_syms_until_next_update = self.update_interval as i32;
    }
}

/// The range decoder. `value` preloads the first four bytes of `buf`
/// big-endian; reads past the end of `buf` return zero bytes.
pub struct ArithDec<'a> {
    buf: &'a [u8],
    cur: usize,
    value: u32,
    length: u32,
}

impl<'a> ArithDec<'a> {
    /// Returns `None` when `buf` is shorter than the five-byte minimum.
    pub fn new(buf: &'a [u8]) -> Option<Self> {
        if buf.len() < ARITH_MIN_EXPECTED_DATA_BUF_SIZE {
            return None;
        }
        Some(Self {
            buf,
            cur: 4,
            value: u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]),
            length: u32::MAX,
        })
    }

    /// Pull input bytes into `value` until `length` is back above the minimum;
    /// input read past the end contributes zero bytes.
    #[inline]
    fn renorm(&mut self) {
        loop {
            let next = if self.cur < self.buf.len() {
                let b = self.buf[self.cur];
                self.cur += 1;
                b as u32
            } else {
                0
            };
            self.value = (self.value << 8) | next;
            self.length <<= 8;
            if self.length >= ARITH_MIN_LEN {
                break;
            }
        }
    }

    /// Decode one raw equiprobable bit.
    pub fn get_bit(&mut self) -> u32 {
        self.length >>= 1;
        let bit = u32::from(self.value >= self.length);
        if bit != 0 {
            self.value -= self.length;
        }
        if self.length < ARITH_MIN_LEN {
            self.renorm();
        }
        bit
    }

    /// Decode `num_bits` (1..=20) raw equiprobable bits as a single value.
    pub fn get_bits(&mut self, num_bits: u32) -> u32 {
        debug_assert!((1..=20).contains(&num_bits));
        self.length >>= num_bits;
        let v = self.value / self.length;
        self.value -= self.length * v;
        if self.length < ARITH_MIN_LEN {
            self.renorm();
        }
        v
    }

    /// Decode one symbol in `0..n` with a truncated binary code (`n >= 2`),
    /// which spends one fewer bit on the low-numbered symbols.
    pub fn decode_truncated_binary(&mut self, n: u32) -> u32 {
        debug_assert!(n >= 2);
        let k = 31 - n.leading_zeros();
        let u = (1u32 << (k + 1)) - n;
        let mut result = if k == 0 { 0 } else { self.get_bits(k) };
        if result >= u {
            result = ((result << 1) | self.get_bits(1)) - u;
        }
        result
    }

    /// Decode one bit against an adaptive binary model, then update the model.
    pub fn decode_bit(&mut self, bm: &mut BitModel) -> u32 {
        let x = bm.bit0_prob * (self.length >> BM_LEN_SHIFT);
        let bit = u32::from(self.value >= x);
        if bit == 0 {
            self.length = x;
            bm.bit0_count += 1;
        } else {
            self.value -= x;
            self.length -= x;
        }
        bm.bit_count += 1;
        if self.length < ARITH_MIN_LEN {
            self.renorm();
        }
        bm.bits_until_update -= 1;
        if bm.bits_until_update <= 0 {
            bm.update();
        }
        bit
    }

    /// Decode an adaptive Exp-Golomb value: an adaptive unary prefix gives the
    /// bit count, then that many adaptive tail bits fill in the value. Returns
    /// 0 if the prefix runs past 16 bits.
    pub fn decode_gamma(&mut self, ctxs: &mut GammaContexts) -> u32 {
        let mut k = 0usize;
        while self.decode_bit(&mut ctxs.prefix[k.min(2)]) != 0 {
            k += 1;
            if k > 16 {
                return 0;
            }
        }
        let mut n = 1u32 << k;
        for i in (0..k).rev() {
            let bit = self.decode_bit(&mut ctxs.tail[i.min(3)]);
            n |= bit << i;
        }
        n
    }

    /// Decode one symbol by binary-searching the snapshot cumulative table,
    /// then bump its histogram frequency and refresh the snapshot when due.
    pub fn decode_sym(&mut self, dm: &mut DataModel) -> u32 {
        let mut x = 0u32;
        let mut y = self.length;
        self.length >>= DM_LEN_SHIFT;
        let mut low_idx = 0usize;
        let mut hi_idx = dm.num_data_syms as usize;
        let mut mid_idx = hi_idx >> 1;
        loop {
            let z = self.length * dm.cum_sym_freqs[mid_idx];
            if z > self.value {
                hi_idx = mid_idx;
                y = z;
            } else {
                low_idx = mid_idx;
                x = z;
            }
            mid_idx = (low_idx + hi_idx) >> 1;
            if mid_idx == low_idx {
                break;
            }
        }
        self.value -= x;
        self.length = y - x;
        if self.length < ARITH_MIN_LEN {
            self.renorm();
        }
        dm.sym_freqs[low_idx] += 1;
        dm.total_sym_freq += 1;
        dm.num_syms_until_next_update -= 1;
        if dm.num_syms_until_next_update <= 0 {
            dm.update();
        }
        low_idx as u32
    }
}

/// LSB-first sub-byte reader: 1/2/4-bit reads that never cross a byte, drawn
/// from a sentinel-tagged bit buffer, while `get_bits8` bypasses the buffer and
/// takes a whole byte. Reads past the end of `buf` return zero.
pub struct SimplifiedDecoder<'a> {
    buf: &'a [u8],
    pos: usize,
    bit_buf: u32,
}

impl<'a> SimplifiedDecoder<'a> {
    /// Start a reader over `buf` with an empty (sentinel-only) bit buffer.
    pub fn new(buf: &'a [u8]) -> Self {
        Self {
            buf,
            pos: 0,
            bit_buf: 1,
        }
    }

    /// Load the next byte into the bit buffer once only the sentinel bit is
    /// left; past the end it loads a zero byte.
    #[inline]
    fn refill(&mut self) {
        if self.bit_buf <= 1 {
            let b = if self.pos < self.buf.len() {
                let v = self.buf[self.pos];
                self.pos += 1;
                v as u32
            } else {
                0
            };
            self.bit_buf = 256 | b;
        }
    }

    /// Read one bit, least significant first.
    #[inline]
    pub fn get_bits1(&mut self) -> u32 {
        self.refill();
        let res = self.bit_buf & 1;
        self.bit_buf >>= 1;
        res
    }

    /// Read two bits, least significant first.
    #[inline]
    pub fn get_bits2(&mut self) -> u32 {
        self.refill();
        let res = self.bit_buf & 3;
        self.bit_buf >>= 2;
        res
    }

    /// Read four bits, least significant first.
    #[inline]
    pub fn get_bits4(&mut self) -> u32 {
        self.refill();
        let res = self.bit_buf & 15;
        self.bit_buf >>= 4;
        res
    }

    /// Whole-byte read, bypassing the sub-byte bit buffer.
    #[inline]
    pub fn get_bits8(&mut self) -> u32 {
        if self.pos < self.buf.len() {
            let v = self.buf[self.pos];
            self.pos += 1;
            v as u32
        } else {
            0
        }
    }

    /// True when every source byte has been consumed. The full-zstd path uses
    /// this to confirm the mode channel was drained exactly.
    pub fn fully_consumed(&self) -> bool {
        self.pos == self.buf.len()
    }
}
