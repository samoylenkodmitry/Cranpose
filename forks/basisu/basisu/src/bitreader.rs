//! UASTC block bit readers (`read_bits1_to_9`, `read_bits64`, `read_bit`).
//! They read little-endian bit fields out of a byte buffer, advancing a bit
//! cursor. The reads are byte-wise, so the result does not depend on host
//! endianness or pointer alignment.

/// Read `codesize` (0..=9) bits at `*bit_offset`, advancing it. Reads at most
/// two bytes from `buf`, so `buf` must hold byte `(bit_offset >> 3) + 1` when
/// the field straddles a byte boundary.
#[inline]
pub fn read_bits1_to_9(buf: &[u8], bit_offset: &mut u32, codesize: u32) -> u32 {
    debug_assert!(codesize <= 9);
    if codesize == 0 {
        return 0;
    }
    let p = (*bit_offset >> 3) as usize;
    let byte_bit_offset = *bit_offset & 7;
    let mut bits = (buf[p] >> byte_bit_offset) as u32;
    let bits_read = codesize.min(8 - byte_bit_offset);
    let bits_remaining = codesize - bits_read;
    if bits_remaining != 0 {
        bits |= (buf[p + 1] as u32) << bits_read;
    }
    *bit_offset += codesize;
    bits & ((1u32 << codesize) - 1)
}

/// Alias of [`read_bits1_to_9`]. A separate entry point is kept for the call
/// sites that read within the first 14 bytes (`bit_offset < 112`); that tighter
/// bound does not change the computation, so the body just delegates.
#[inline]
pub fn read_bits1_to_9_fst(buf: &[u8], bit_offset: &mut u32, codesize: u32) -> u32 {
    read_bits1_to_9(buf, bit_offset, codesize)
}

/// Read a single bit at `*bit_offset`, advancing it by 1.
#[inline]
pub fn read_bit(buf: &[u8], bit_offset: &mut u32) -> u32 {
    let byte_bits = buf[(*bit_offset >> 3) as usize] >> (*bit_offset & 7);
    *bit_offset += 1;
    (byte_bits & 1) as u32
}

/// Read `codesize` (0..=64) bits at `*bit_offset`, advancing it. Byte-wise, so
/// endian-independent.
#[inline]
pub fn read_bits64(buf: &[u8], bit_offset: &mut u32, codesize: u32) -> u64 {
    debug_assert!(codesize <= 64);
    let mut bits = 0u64;
    let mut total_bits = 0u32;
    while total_bits < codesize {
        let byte_bit_offset = *bit_offset & 7;
        let bits_to_read = (codesize - total_bits).min(8 - byte_bit_offset);
        let mut byte_bits = (buf[(*bit_offset >> 3) as usize] >> byte_bit_offset) as u32;
        byte_bits &= (1u32 << bits_to_read) - 1;
        bits |= (byte_bits as u64) << total_bits;
        total_bits += bits_to_read;
        *bit_offset += bits_to_read;
    }
    bits
}
