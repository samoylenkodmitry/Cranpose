use super::*;

#[test]
fn the_fade_is_the_spring_and_the_delay_wear_declares() {
    assert_eq!(INDICATOR_IDLE_DELAY_MS, 2000);
    assert_eq!(INDICATOR_FADE_STIFFNESS, 400.0, "StiffnessMediumLow");
    assert_eq!(INDICATOR_FADE_DAMPING_RATIO, 1.0);
}

#[test]
fn a_scaffold_shows_no_indicator_until_one_is_asked_for() {
    assert!(!ScreenScaffoldSpec::default().show_indicator);
    assert!(
        ScreenScaffoldSpec::default()
            .indicator(ScrollIndicatorSpec::default())
            .show_indicator
    );
}
