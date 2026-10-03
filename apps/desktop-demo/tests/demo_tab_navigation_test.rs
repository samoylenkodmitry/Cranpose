use cranpose_testing::robot::{RobotTestRule, TestRenderer};
use desktop_app::app::{combined_app, DemoTab, DEFAULT_INITIAL_TAB, DEMO_TABS};

#[test]
fn shared_platform_entry_opens_the_guide() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), combined_app);
    assert!(robot
        .find_by_text("Build apps with Compose in Rust.")
        .exists());
}

#[test]
fn default_tab_is_first_in_navigation() {
    assert_eq!(DEFAULT_INITIAL_TAB, DemoTab::Guide);
    assert_eq!(DEMO_TABS.first(), Some(&DemoTab::Guide));
}

#[test]
fn liquid_ui_is_a_front_row_demo_tab() {
    let liquid_index = DEMO_TABS
        .iter()
        .position(|tab| *tab == DemoTab::Liquid)
        .expect("Liquid UI must be registered in the desktop demo");

    assert!(
        liquid_index <= 2,
        "Liquid UI must be visible beside the default tab without scrolling; index={liquid_index}"
    );
}
