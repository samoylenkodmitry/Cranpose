use cranpose_text::font_feature::font_feature_settings;

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
