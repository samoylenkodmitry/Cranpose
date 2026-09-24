use super::*;

#[test]
fn an_indicator_tells_a_reader_the_app_is_busy() {
    let mut config = cranpose_foundation::SemanticsConfiguration::default();
    busy_semantics(&mut config);
    assert_eq!(config.content_description.as_deref(), Some("Loading"));
    assert_eq!(
        config.role,
        Some(cranpose_foundation::SemanticsWidgetRole::ProgressBar)
    );
    assert!(
        config.progress.is_none(),
        "an indicator with no value must not read as a slider"
    );
}
