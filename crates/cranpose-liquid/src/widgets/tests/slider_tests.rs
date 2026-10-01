use super::*;

#[test]
fn slider_accessibility_exposes_the_controlled_value_without_calling_back() {
    let requested = Rc::new(Cell::new(-1.0));
    let observed = Rc::clone(&requested);
    let mut config = cranpose_ui::SemanticsConfiguration::default();
    slider_semantics(0.25, Rc::new(move |value| observed.set(value)))(&mut config);
    assert_eq!(config.state_description.as_deref(), Some("25%"));
    assert_eq!(
        config.progress,
        Some(cranpose_ui::ProgressBarRangeInfo::new(0.25, 0.0, 1.0, 0))
    );
    assert_eq!(requested.get(), -1.0);
}
