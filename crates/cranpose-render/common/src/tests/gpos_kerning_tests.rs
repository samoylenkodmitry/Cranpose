use super::*;

const NOTO: &[u8] = include_bytes!("../../assets/NotoSansMerged.ttf");

fn face() -> Face<'static> {
    Face::parse(NOTO, 0).expect("font")
}

fn glyph(face: &Face<'_>, ch: char) -> u16 {
    face.glyph_index(ch).expect("glyph").0
}

#[test]
fn embedded_font_has_no_truetype_kern_table() {
    assert!(face().raw_face().table(Tag::from_bytes(b"kern")).is_none());
}

#[test]
fn gpos_kerning_is_found_and_negative_for_a_kerning_pair() {
    let face = face();
    let kerning = GposKerning::parse(&face).expect("GPOS kerning");
    assert!(!kerning.is_empty());
    let value = kerning.kern_unscaled(glyph(&face, 'A'), glyph(&face, 'V'));
    assert!(value < 0.0, "AV should tuck, got {value}");
}

#[test]
fn unkerned_pairs_are_zero() {
    let face = face();
    let kerning = GposKerning::parse(&face).expect("GPOS kerning");
    assert_eq!(
        kerning.kern_unscaled(glyph(&face, 'n'), glyph(&face, 'n')),
        0.0
    );
}

#[test]
fn kerning_is_directional() {
    let face = face();
    let kerning = GposKerning::parse(&face).expect("GPOS kerning");
    let forward = kerning.kern_unscaled(glyph(&face, 'A'), glyph(&face, 'V'));
    let backward = kerning.kern_unscaled(glyph(&face, 'V'), glyph(&face, 'A'));
    assert!(forward < 0.0 && backward < 0.0);
}

#[test]
fn class_def_collapses_runs_and_defaults_to_zero() {
    let classes = ClassDef::from_pairs(vec![(4, 1), (5, 1), (6, 1), (9, 2), (12, 0)]);
    assert_eq!(classes.ranges, vec![(4, 6, 1), (9, 9, 2)]);
    assert_eq!(classes.class_of(5), 1);
    assert_eq!(classes.class_of(9), 2);
    assert_eq!(classes.class_of(12), 0);
    assert_eq!(classes.class_of(0), 0);
}

#[test]
fn value_size_counts_only_the_low_byte() {
    assert_eq!(value_size(0x0000), 0);
    assert_eq!(value_size(0x0004), 2);
    assert_eq!(value_size(0x0044), 4);
    assert_eq!(value_size(0x00FF), 16);
}
