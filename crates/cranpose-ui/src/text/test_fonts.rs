use cranpose_text::{FontFamilyResolver, Typeface};

// Cranpose Test Mono, built by `scripts/fonts/make_test_mono.py`.
const TEST_MONO: &[u8] = include_bytes!("../../assets/CranposeTestMono.ttf");

/// The fonts tests lay text out with: Cranpose Test Mono, whose every
/// character, one it has no glyph for included, advances 0.6 em, and whose
/// lines are 1 em tall with the baseline 0.8 em down, so a test's numbers
/// do not hang on a real font's design.
pub fn test_fonts() -> FontFamilyResolver {
    FontFamilyResolver::new(Typeface::from_bytes(TEST_MONO).ok())
}
