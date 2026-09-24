use super::*;

#[test]
fn resolve_content_direction_detects_rtl_script() {
    assert_eq!(
        TextDirection::Content.resolve("שלום"),
        ResolvedTextDirection::Rtl
    );
}

#[test]
fn resolve_content_direction_detects_ltr_script() {
    assert_eq!(
        TextDirection::Content.resolve("Compose"),
        ResolvedTextDirection::Ltr
    );
}

#[test]
fn resolve_content_or_rtl_falls_back_to_rtl() {
    assert_eq!(
        TextDirection::ContentOrRtl.resolve("12345"),
        ResolvedTextDirection::Rtl
    );
}

#[test]
fn resolve_text_direction_defaults_to_ltr_for_unspecified() {
    assert_eq!(
        resolve_text_direction("12345", None),
        ResolvedTextDirection::Ltr
    );
}

#[test]
fn resolve_text_direction_uses_content_for_unspecified() {
    assert_eq!(
        resolve_text_direction("שלום", Some(TextDirection::Unspecified)),
        ResolvedTextDirection::Rtl
    );
}

#[test]
fn line_break_take_or_else_uses_fallback_for_unspecified() {
    let value = LineBreak::Unspecified.take_or_else(|| LineBreak::Simple);
    assert_eq!(value, LineBreak::Simple);
    assert!(LineBreak::Simple.is_specified());
}

#[test]
fn hyphens_take_or_else_uses_fallback_for_unspecified() {
    let value = Hyphens::Unspecified.take_or_else(|| Hyphens::None);
    assert_eq!(value, Hyphens::None);
    assert!(Hyphens::Auto.is_specified());
}
