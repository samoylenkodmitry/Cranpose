//! Fonts with 65,535 glyphs, the most a font can hold.
//!
//! ttf-parser 0.25 counts the offsets of its per-glyph tables, one more than
//! the glyphs, in a `u16`. At 65,535 glyphs the count overflows and the parser
//! reads no `loca` table, so no glyph has an outline, and no `gvar` table, so a
//! variable font draws only its default instance, the thinnest weight of Noto
//! Sans CJK. Full CJK fonts such as Noto Sans CJK have exactly that many
//! glyphs. These functions shorten both tables by their last offset, which
//! costs the last glyph only.

use std::sync::{Mutex, PoisonError};

use cranpose_text::FontBytes;

/// Copies of fonts the app holds for the life of the process, shortened,
/// each with the address of its source.
static FITTED: Mutex<Vec<(usize, &'static [u8])>> = Mutex::new(Vec::new());

// `bytes` with per-glyph tables ttf-parser reads.
pub(crate) fn fitted(bytes: FontBytes) -> FontBytes {
    match bytes {
        FontBytes::Static(bytes) => FontBytes::Static(fit_static_glyph_tables(bytes)),
        FontBytes::Owned(mut bytes) => {
            fit_glyph_tables(&mut bytes);
            FontBytes::Owned(bytes)
        }
    }
}

/// Shortens the per-glyph tables of `bytes` in place where ttf-parser would
/// read none of them.
pub(crate) fn fit_glyph_tables(bytes: &mut [u8]) {
    for (at, value) in fits(bytes).into_iter().flatten() {
        value.write(bytes, at);
    }
}

/// `bytes`, or for a font with a per-glyph table ttf-parser would not read,
/// one shortened copy kept for the life of the process.
pub(crate) fn fit_static_glyph_tables(bytes: &'static [u8]) -> &'static [u8] {
    if fits(bytes).iter().all(Option::is_none) {
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
    fit_glyph_tables(&mut copy);
    let copy: &'static [u8] = Box::leak(copy.into_boxed_slice());
    fitted.push((source, copy));
    copy
}

/// A field to write: a table length or a glyph count.
#[derive(Clone, Copy)]
enum Value {
    U16(u16),
    U32(u32),
}

impl Value {
    fn write(self, bytes: &mut [u8], at: usize) {
        match self {
            Self::U16(value) => write(bytes, at, value.to_be_bytes()),
            Self::U32(value) => write(bytes, at, value.to_be_bytes()),
        }
    }
}

fn write<const N: usize>(bytes: &mut [u8], at: usize, value: [u8; N]) {
    if let Some(field) = bytes.get_mut(at..at + N) {
        field.copy_from_slice(&value);
    }
}

/// Where in the first face of `bytes` a value is to be written, and the value,
/// for the `loca` table and for the `gvar` table.
fn fits(bytes: &[u8]) -> [Option<(usize, Value)>; 2] {
    let Some(tables) = Tables::read(bytes) else {
        return [None, None];
    };
    [tables.loca_fit(), tables.gvar_fit()]
}

/// The table directory of the first face of a font.
struct Tables<'a> {
    bytes: &'a [u8],
    directory: usize,
    count: usize,
}

impl<'a> Tables<'a> {
    fn read(bytes: &'a [u8]) -> Option<Self> {
        // A collection names where each face's table directory starts.
        let directory = if bytes.get(0..4)? == b"ttcf" {
            usize::try_from(read_u32(bytes, 12)?).ok()?
        } else {
            0
        };
        let count = usize::from(read_u16(bytes, directory + 4)?);
        Some(Self {
            bytes,
            directory,
            count,
        })
    }

    /// Where the record of the table `tag` starts.
    fn record(&self, tag: &[u8; 4]) -> Option<usize> {
        (0..self.count)
            .map(|index| self.directory + 12 + 16 * index)
            .find(|&at| self.bytes.get(at..at + 4) == Some(tag.as_slice()))
    }

    /// Where the table `tag` starts.
    fn table(&self, tag: &[u8; 4]) -> Option<usize> {
        usize::try_from(read_u32(self.bytes, self.record(tag)? + 8)?).ok()
    }

    /// The `loca` table's length, cut to the 65,535 offsets ttf-parser reads.
    fn loca_fit(&self) -> Option<(usize, Value)> {
        // `indexToLocFormat`: short offsets take two bytes, long ones four.
        let offset_size = if read_u16(self.bytes, self.table(b"head")? + 50)? == 0 {
            2
        } else {
            4
        };
        let length_at = self.record(b"loca")? + 12;
        let readable = u32::from(u16::MAX) * offset_size;
        (read_u32(self.bytes, length_at)? > readable).then_some((length_at, Value::U32(readable)))
    }

    /// The `gvar` table's glyph count, one less when its offsets would number
    /// 65,536.
    fn gvar_fit(&self) -> Option<(usize, Value)> {
        let count_at = self.table(b"gvar")? + 12;
        (read_u16(self.bytes, count_at)? == u16::MAX)
            .then_some((count_at, Value::U16(u16::MAX - 1)))
    }
}

fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}
