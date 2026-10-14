//! Fonts whose `loca` table holds more offsets than ttf-parser counts.
//!
//! ttf-parser 0.25 counts the offsets of a `loca` table in a `u16` and reads
//! no table at all when there are more, so every glyph of such a font loses
//! its outline: its text measures right and draws blank. A font with 65,535
//! glyphs, the most a font can have, has 65,536 offsets; full CJK fonts such as
//! Noto Sans CJK have exactly that many glyphs. These functions shorten the
//! table to 65,535 offsets, which costs the outline of the last glyph only.

use std::sync::{Mutex, PoisonError};

/// Copies of fonts the app holds for the life of the process, shortened,
/// each with the address of its source.
static FITTED: Mutex<Vec<(usize, &'static [u8])>> = Mutex::new(Vec::new());

/// Shortens the `loca` table of `bytes` in place when ttf-parser would read
/// none of it.
pub(crate) fn fit_loca(bytes: &mut [u8]) {
    let Some((at, length)) = readable_loca(bytes) else {
        return;
    };
    if let Some(field) = bytes.get_mut(at..at + 4) {
        field.copy_from_slice(&length.to_be_bytes());
    }
}

/// `bytes`, or for a font whose `loca` table ttf-parser would not read, one
/// shortened copy kept for the life of the process.
pub(crate) fn fit_static_loca(bytes: &'static [u8]) -> &'static [u8] {
    if readable_loca(bytes).is_none() {
        return bytes;
    }
    let source = bytes.as_ptr() as usize;
    let mut fitted = FITTED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((_, copy)) = fitted
        .iter()
        .find(|(address, copy)| *address == source && copy.len() == bytes.len())
    {
        return copy;
    }
    let mut copy = bytes.to_vec();
    fit_loca(&mut copy);
    let copy: &'static [u8] = Box::leak(copy.into_boxed_slice());
    fitted.push((source, copy));
    copy
}

/// Where the length of the first face's `loca` table is written in `bytes`,
/// and the length ttf-parser reads, when the table is longer than that.
fn readable_loca(bytes: &[u8]) -> Option<(usize, u32)> {
    // A collection names where each face's table directory starts.
    let directory = if bytes.get(0..4)? == b"ttcf" {
        usize::try_from(read_u32(bytes, 12)?).ok()?
    } else {
        0
    };
    let tables = usize::from(read_u16(bytes, directory + 4)?);
    let record = |tag: &[u8; 4]| {
        (0..tables)
            .map(|index| directory + 12 + 16 * index)
            .find(|&at| bytes.get(at..at + 4) == Some(tag.as_slice()))
    };
    let head = usize::try_from(read_u32(bytes, record(b"head")? + 8)?).ok()?;
    // `indexToLocFormat`: short offsets take two bytes, long ones four.
    let offset_size = if read_u16(bytes, head + 50)? == 0 {
        2
    } else {
        4
    };
    let length_at = record(b"loca")? + 12;
    let readable = u32::from(u16::MAX) * offset_size;
    (read_u32(bytes, length_at)? > readable).then_some((length_at, readable))
}

fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}
