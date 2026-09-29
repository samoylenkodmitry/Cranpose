use ab_glyph::{Font, FontArc};

use super::{ASCII_COUNT, AsciiGlyphs, ascii_slot};
use crate::gpos_kerning::KernedFont;

const FONT: &[u8] = include_bytes!("../../assets/NotoSansMerged.ttf");

/// The bundled face with its GPOS kerning, which kerns `AV` and `To`.
fn font() -> KernedFont {
    KernedFont::new(
        FontArc::try_from_slice(FONT).expect("font"),
        KernedFont::read_kerning(FONT, &[]),
    )
}

fn printable() -> impl Iterator<Item = char> {
    (' '..='~').filter(|ch| ascii_slot(*ch).is_some())
}

#[test]
fn every_printable_ascii_character_has_a_slot_and_nothing_else_does() {
    assert_eq!(printable().count(), ASCII_COUNT);
    for ch in ['\n', '\t', '\u{7f}', 'é', '→', '中'] {
        assert_eq!(ascii_slot(ch), None, "{ch:?} is not printable ASCII");
    }
}

#[test]
fn a_glyph_read_from_the_table_is_the_fonts_own_on_first_and_later_reads() {
    let font = font();
    let table = AsciiGlyphs::new();
    for _ in 0..2 {
        for ch in printable() {
            let id = font.glyph_id(ch);
            assert_eq!(
                table.glyph(&font, ch),
                Some((id, font.h_advance_unscaled(id))),
                "{ch:?}"
            );
        }
    }
}

#[test]
fn a_character_outside_printable_ascii_is_left_to_the_font() {
    let font = font();
    let table = AsciiGlyphs::new();
    assert_eq!(table.glyph(&font, 'é'), None);
    let e = (' ', font.glyph_id(' '));
    let acute = ('é', font.glyph_id('é'));
    assert_eq!(table.kern(&font, e, acute), None);
    assert_eq!(table.kern(&font, acute, e), None);
}

#[test]
fn a_pair_kerned_from_the_table_is_the_fonts_own_on_first_and_later_reads() {
    let font = font();
    let table = AsciiGlyphs::new();
    let with_id = |ch: char| (ch, font.glyph_id(ch));
    let mut kerned_pairs = 0usize;
    for _ in 0..2 {
        for previous in printable() {
            for ch in printable() {
                let expected = font.kern_unscaled(font.glyph_id(previous), font.glyph_id(ch));
                assert_eq!(
                    table.kern(&font, with_id(previous), with_id(ch)),
                    Some(expected),
                    "{previous:?}{ch:?}"
                );
                kerned_pairs += usize::from(expected != 0.0);
            }
        }
    }
    assert!(kerned_pairs > 0, "the face kerns some ASCII pairs");
}

#[test]
fn threads_filling_the_table_at_once_agree_with_the_font() {
    let font = font();
    let table = AsciiGlyphs::new();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                for previous in printable() {
                    for ch in printable() {
                        let pair = ((previous, font.glyph_id(previous)), (ch, font.glyph_id(ch)));
                        assert_eq!(
                            table.kern(&font, pair.0, pair.1),
                            Some(font.kern_unscaled(pair.0.1, pair.1.1))
                        );
                    }
                    let id = font.glyph_id(previous);
                    assert_eq!(
                        table.glyph(&font, previous),
                        Some((id, font.h_advance_unscaled(id)))
                    );
                }
            });
        }
    });
}
