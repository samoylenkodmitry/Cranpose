//! Ogg's CRC-32 (RFC 3533 §6).
//!
//! Not the common CRC-32: Ogg uses the same polynomial as Ethernet but runs it
//! **non-reflected**, with a zero initial value and no final inversion. Feeding
//! Ogg data to a stock `crc32` gives a different answer, which is why this lives
//! here rather than deferring to a dependency.

const POLY: u32 = 0x04c1_1db7;

/// MSB-first table, generated once at compile time.
const TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut r = (i as u32) << 24;
        let mut bit = 0;
        while bit < 8 {
            r = if r & 0x8000_0000 != 0 {
                (r << 1) ^ POLY
            } else {
                r << 1
            };
            bit += 1;
        }
        table[i] = r;
        i += 1;
    }
    table
};

/// Running Ogg CRC-32 over a page, which is checksummed with its own CRC field
/// zeroed — see [`super::page::PageHeader`].
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Crc32(u32);

impl Crc32 {
    pub(crate) fn new() -> Self {
        Crc32(0)
    }

    pub(crate) fn update(&mut self, data: &[u8]) {
        for &b in data {
            let idx = ((self.0 >> 24) as u8 ^ b) as usize;
            self.0 = (self.0 << 8) ^ TABLE[idx];
        }
    }

    /// Feed `n` zero bytes — how the CRC field itself is covered.
    pub(crate) fn update_zeros(&mut self, n: usize) {
        for _ in 0..n {
            let idx = (self.0 >> 24) as usize;
            self.0 = (self.0 << 8) ^ TABLE[idx];
        }
    }

    pub(crate) fn finish(self) -> u32 {
        self.0
    }
}

/// One-shot CRC over a contiguous buffer.
pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut c = Crc32::new();
    c.update(data);
    c.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table-driven implementation must agree with the bit-at-a-time
    /// definition straight out of RFC 3533 — the table is the only thing that
    /// could silently drift, so it gets its own oracle.
    fn crc32_bitwise(data: &[u8]) -> u32 {
        let mut crc: u32 = 0;
        for &b in data {
            crc ^= (b as u32) << 24;
            for _ in 0..8 {
                crc = if crc & 0x8000_0000 != 0 {
                    (crc << 1) ^ POLY
                } else {
                    crc << 1
                };
            }
        }
        crc
    }

    #[test]
    fn table_matches_bitwise_oracle() {
        let mut state = 0x1234_5678u32;
        for len in 0..300usize {
            let data: Vec<u8> = (0..len)
                .map(|_| {
                    state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (state >> 24) as u8
                })
                .collect();
            assert_eq!(crc32(&data), crc32_bitwise(&data), "len {len}");
        }
    }

    #[test]
    fn empty_input_is_zero() {
        assert_eq!(crc32(&[]), 0);
    }

    #[test]
    fn update_is_incremental() {
        let data: Vec<u8> = (0u8..=255).collect();
        for split in [0, 1, 17, 128, 255, 256] {
            let mut c = Crc32::new();
            c.update(&data[..split]);
            c.update(&data[split..]);
            assert_eq!(c.finish(), crc32(&data), "split at {split}");
        }
    }

    #[test]
    fn update_zeros_matches_explicit_zero_bytes() {
        let mut a = Crc32::new();
        a.update(b"OggS");
        a.update_zeros(4);
        a.update(b"tail");

        let mut b = Crc32::new();
        b.update(b"OggS");
        b.update(&[0, 0, 0, 0]);
        b.update(b"tail");

        assert_eq!(a.finish(), b.finish());
    }
}
