use super::*;

const NOTO: &[u8] = include_bytes!("../../assets/NotoSansMerged.ttf");

fn face() -> Face<'static> {
    Face::parse(NOTO, 0).expect("font")
}

fn features(settings: &str) -> Vec<(String, u32)> {
    font_feature_settings(settings)
        .map(|feature| {
            (
                String::from_utf8_lossy(&feature.tag).into_owned(),
                feature.value,
            )
        })
        .collect()
}

fn expected(entries: &[(&str, u32)]) -> Vec<(String, u32)> {
    entries
        .iter()
        .map(|&(tag, value)| (tag.to_owned(), value))
        .collect()
}

fn glyph(face: &Face<'_>, ch: char) -> ab_glyph::GlyphId {
    ab_glyph::GlyphId(face.glyph_index(ch).expect("glyph").0)
}

fn substitutions(settings: &str) -> Option<GlyphSubstitutions> {
    GlyphSubstitutions::build(&face(), &enabled_feature_tags(settings))
}

#[test]
fn a_bare_tag_is_on_whether_double_single_or_not_quoted() {
    for settings in ["\"smcp\"", "'smcp'", "smcp"] {
        assert_eq!(features(settings), expected(&[("smcp", 1)]), "{settings}");
    }
}

#[test]
fn on_and_off_are_one_and_zero_in_any_case() {
    assert_eq!(features("\"smcp\" on"), expected(&[("smcp", 1)]));
    assert_eq!(features("\"smcp\" off"), expected(&[("smcp", 0)]));
    assert_eq!(
        features("'zero' ON, 'zero' Off"),
        expected(&[("zero", 1), ("zero", 0)])
    );
}

#[test]
fn an_integer_is_the_value_and_zero_is_off() {
    assert_eq!(features("\"pnum\" 1"), expected(&[("pnum", 1)]));
    assert_eq!(features("\"pnum\" 0"), expected(&[("pnum", 0)]));
    assert_eq!(features("\"salt\" 3"), expected(&[("salt", 3)]));
}

#[test]
fn entries_are_comma_separated_and_kept_in_order() {
    assert_eq!(
        features("\"smcp\", \"zero\" 0, \"onum\" on, ss01"),
        expected(&[("smcp", 1), ("zero", 0), ("onum", 1), ("ss01", 1)])
    );
}

#[test]
fn spacing_around_tags_values_and_commas_is_ignored() {
    assert_eq!(
        features("  \"smcp\"   on ,\t'zero'\n,  tnum  ,\"lnum\"0"),
        expected(&[("smcp", 1), ("zero", 1), ("tnum", 1), ("lnum", 0)])
    );
}

#[test]
fn invalid_entries_are_skipped_without_panicking() {
    for settings in [
        "",
        ",",
        " , ,",
        "\"smc\"",
        "\"smcpx\"",
        "\"smcp",
        "'smcp\"",
        "smc",
        "smcpx",
        "smcp=1",
        "\"smcp\" maybe",
        "\"smcp\" -1",
        "\"smcp\" +1",
        "\"smcp\" 1.5",
        "\"smcp\" 99999999999",
        "\"smcp\" on on",
        "\"sé\"",
        "\"é\"",
        "\"\u{1F600}\"",
        "\u{1F600}",
    ] {
        assert_eq!(features(settings), Vec::new(), "{settings:?}");
    }
    assert_eq!(
        features("\"smcp\", bogus, \"zero\" 0, \"tnum\" x, 'pnum'"),
        expected(&[("smcp", 1), ("zero", 0), ("pnum", 1)])
    );
}

#[test]
fn enabled_tags_take_each_tags_last_value_sorted() {
    assert_eq!(
        enabled_feature_tags("\"zero\", \"smcp\" 0, \"pnum\", \"smcp\", \"onum\" 1, \"onum\" 0"),
        vec![*b"pnum", *b"smcp", *b"zero"]
    );
    assert!(enabled_feature_tags("\"pnum\" 0").is_empty());
    assert!(enabled_feature_tags("nonsense").is_empty());
}

#[test]
fn pnum_maps_tabular_digits_to_harfbuzzs_proportional_figures() {
    let face = face();
    let pnum = substitutions("pnum").expect("pnum substitutes");
    assert_eq!(pnum.apply(glyph(&face, '0')), ab_glyph::GlyphId(2241));
    assert_eq!(pnum.apply(glyph(&face, '1')), ab_glyph::GlyphId(2242));
    assert_eq!(pnum.apply(glyph(&face, 'a')), glyph(&face, 'a'));
    assert_eq!(pnum.tags(), [*b"pnum"]);
    assert_ne!(pnum.key(), 0);
}

#[test]
fn features_apply_in_lookup_list_order_like_harfbuzz() {
    let face = face();
    let figures = substitutions("\"onum\", \"pnum\"").expect("figures substitute");
    assert_eq!(figures.apply(glyph(&face, '0')), ab_glyph::GlyphId(2231));
    assert_eq!(figures.apply(glyph(&face, '1')), ab_glyph::GlyphId(2232));
    let small_caps = substitutions("smcp, zero").expect("small caps substitute");
    assert_eq!(small_caps.apply(glyph(&face, 'a')), ab_glyph::GlyphId(1868));
    assert_eq!(small_caps.apply(glyph(&face, 'é')), ab_glyph::GlyphId(1893));
    assert_eq!(small_caps.apply(glyph(&face, '0')), ab_glyph::GlyphId(2251));
    assert_ne!(figures.key(), small_caps.key());
}

#[test]
fn features_with_other_lookup_types_are_not_applied() {
    for settings in [
        "liga", "dlig", "ccmp", "frac", "ordn", "aalt", "kern", "xxxx",
    ] {
        assert!(substitutions(settings).is_none(), "{settings}");
    }
}

#[test]
fn variants_are_built_once_per_settings_and_shared_by_equal_feature_sets() {
    let variants = FeatureVariants::new(Arc::new(AsciiGlyphs::new()));
    let first = variants.get("\"zero\"", NOTO).expect("zero");
    let again = variants.get("\"zero\"", NOTO).expect("zero");
    let spelled = variants.get("'zero' on, smcp 0", NOTO).expect("zero");
    assert!(Arc::ptr_eq(&first.substitutions, &again.substitutions));
    assert!(Arc::ptr_eq(&first.ascii, &again.ascii));
    assert!(Arc::ptr_eq(&first.substitutions, &spelled.substitutions));
    assert!(variants.get("\"zero\" 0", NOTO).is_none());
    assert!(variants.get("liga", NOTO).is_none());
}

#[test]
fn variants_keep_a_bounded_number_of_settings() {
    let variants = FeatureVariants::new(Arc::new(AsciiGlyphs::new()));
    for value in 0..=MAX_FEATURE_VARIANTS {
        let settings = format!("\"zero\" {}", value + 1);
        assert!(variants.get(&settings, NOTO).is_some());
    }
    let held = variants.variants.lock().expect("variants");
    assert_eq!(held.len(), MAX_FEATURE_VARIANTS);
    assert_eq!(&*held[0].0, "\"zero\" 2");
}

#[test]
fn base_ascii_is_the_table_the_variants_were_made_with() {
    let base = Arc::new(AsciiGlyphs::new());
    let variants = FeatureVariants::new(Arc::clone(&base));
    assert!(Arc::ptr_eq(variants.base_ascii(), &base));
    let zero = variants.get("zero", NOTO).expect("zero");
    assert!(!Arc::ptr_eq(&zero.ascii, &base));
}
