use super::*;

#[test]
fn search_spec_defaults_to_a_search_label_and_glass() {
    let spec = LiquidSearchFieldSpec::default();
    assert_eq!(spec.placeholder, "Search");
    assert!(spec.on_glass);
    assert_eq!(spec.glass, None);
    assert_eq!(spec.foreground, None);
}
