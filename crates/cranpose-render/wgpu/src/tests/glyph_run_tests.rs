use super::*;

fn placement(glyph_id: u32, x: i32, font_hash: u64, color: Color) -> SoftwareGlyphAtlasPlacement {
    SoftwareGlyphAtlasPlacement {
        key: SoftwareGlyphAtlasKey {
            font_hash,
            glyph_id,
            scale_x_bits: 14.0f32.to_bits(),
            scale_y_bits: 14.0f32.to_bits(),
            embolden_px_bits: 0,
            slant_bits: 0,
        },
        x,
        y: -3,
        width: 7,
        height: 9,
        color,
    }
}

fn fields(
    glyph: SoftwareGlyphAtlasPlacement,
) -> (SoftwareGlyphAtlasKey, i32, i32, usize, usize, Color) {
    (
        glyph.key,
        glyph.x,
        glyph.y,
        glyph.width,
        glyph.height,
        glyph.color,
    )
}

const INK: Color = Color(0.1, 0.2, 0.3, 1.0);
const LINK: Color = Color(0.0, 0.4, 1.0, 1.0);

#[test]
fn a_run_gives_back_the_placements_it_was_made_from() {
    // Two segments of one text: a word in ink, then one in the link color
    // and another face, then ink again.
    let placements = [
        placement(40, 0, 7, INK),
        placement(41, 8, 7, INK),
        placement(42, 16, 9, LINK),
        placement(43, 24, 7, INK),
    ];
    let mut scratch = RunGlyphScratch::default();
    let run = RunGlyphs::of(placements.iter().copied(), &mut scratch).expect("compact");
    assert_eq!(run.iter().len(), placements.len());
    for (glyph, placement) in run.iter().zip(placements) {
        assert_eq!(fields(glyph), fields(placement));
    }
    assert_eq!(run.faces.len(), 3, "a face per change of segment");
}

#[test]
fn runs_in_the_same_faces_share_them() {
    let mut scratch = RunGlyphScratch::default();
    let first =
        RunGlyphs::of([placement(40, 0, 7, INK)].into_iter(), &mut scratch).expect("compact");
    let second =
        RunGlyphs::of([placement(41, 0, 7, INK)].into_iter(), &mut scratch).expect("compact");
    let other =
        RunGlyphs::of([placement(41, 0, 7, LINK)].into_iter(), &mut scratch).expect("compact");
    assert!(Rc::ptr_eq(&first.faces, &second.faces));
    assert!(!Rc::ptr_eq(&second.faces, &other.faces));
}

#[test]
fn a_glyph_past_the_compact_form_has_no_run() {
    let mut scratch = RunGlyphScratch::default();
    let huge = SoftwareGlyphAtlasPlacement {
        width: 70_000,
        ..placement(40, 0, 7, INK)
    };
    assert!(RunGlyphs::of([huge].into_iter(), &mut scratch).is_none());
    let far_id = placement(70_000, 0, 7, INK);
    assert!(RunGlyphs::of([far_id].into_iter(), &mut scratch).is_none());
}

#[test]
fn a_compact_glyph_takes_sixteen_bytes() {
    assert_eq!(std::mem::size_of::<RunGlyph>(), 16);
    assert!(std::mem::size_of::<SoftwareGlyphAtlasPlacement>() >= 64);
}
