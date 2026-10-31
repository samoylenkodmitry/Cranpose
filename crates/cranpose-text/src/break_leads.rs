// The first byte of `bytes` from `from` on that may start a hard line break
// or a paragraph separator in UTF-8: an ASCII control character, or 0xC2 or
// 0xE2, which start next line and the line and paragraph separators. Text
// reads eight bytes at a time until a word holds one.
pub(crate) fn find_break_lead(bytes: &[u8], from: usize) -> Option<usize> {
    const ONES: u64 = u64::from_ne_bytes([0x01; 8]);
    const HIGH: u64 = u64::from_ne_bytes([0x80; 8]);
    let rest = bytes.get(from..)?;
    let mut skipped = 0;
    for &chunk in rest.as_chunks::<8>().0 {
        let word = u64::from_ne_bytes(chunk);
        // A byte under 0x20 borrows into its top bit when 0x20 is taken
        // from it; 0xC2 and 0xE2 turn zero once bit 5 is cleared and 0xC2
        // taken off, and a zero byte borrows into its top bit when one is
        // taken from it.
        let lead = (word & !(ONES * 0x20)) ^ (ONES * 0xC2);
        let control = word.wrapping_sub(ONES * 0x20) & !word;
        if (control | (lead.wrapping_sub(ONES) & !lead)) & HIGH != 0 {
            break;
        }
        skipped += 8;
    }
    rest[skipped..]
        .iter()
        .position(|&byte| byte < 0x20 || byte & !0x20 == 0xC2)
        .map(|found| from + skipped + found)
}
