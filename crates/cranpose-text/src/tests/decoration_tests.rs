use super::*;

#[test]
fn contains_requires_all_bits_from_other_decoration() {
    let combined = TextDecoration::UNDERLINE.combine(TextDecoration::LINE_THROUGH);

    assert!(combined.contains(TextDecoration::UNDERLINE));
    assert!(combined.contains(TextDecoration::LINE_THROUGH));
    assert!(combined.contains(combined));
    assert!(!TextDecoration::UNDERLINE.contains(combined));
    assert!(!TextDecoration::LINE_THROUGH.contains(combined));
}

#[test]
fn none_contains_only_none() {
    assert!(TextDecoration::NONE.contains(TextDecoration::NONE));
    assert!(TextDecoration::UNDERLINE.contains(TextDecoration::NONE));
    assert!(!TextDecoration::NONE.contains(TextDecoration::UNDERLINE));
    assert!(!TextDecoration::NONE.contains(TextDecoration::LINE_THROUGH));
}
