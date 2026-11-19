use super::*;

#[test]
fn spaces_keep_each_glyph_once_as_they_grow() {
    let mut spaces = Spaces::of(GlyphId::new(40));
    let new: Vec<bool> = [90, 7, 40, 63, 7, 1000, 2, 63, 31]
        .into_iter()
        .map(|glyph| spaces.insert(GlyphId::new(glyph)))
        .collect();
    assert_eq!(
        new,
        [true, true, false, true, false, true, true, false, true]
    );
    let mut seen: Vec<u32> = spaces.iter().map(GlyphId::to_u32).collect();
    seen.sort_unstable();
    assert_eq!(seen, [2, 7, 31, 40, 63, 90, 1000]);
    assert!(spaces.contains(GlyphId::new(31)));
    assert!(!spaces.contains(GlyphId::new(32)));
}
