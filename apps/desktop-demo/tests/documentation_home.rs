use cranpose_testing::robot::{RobotTestRule, TestRenderer};
use desktop_app::app::{combined_app_with_initial_tab, startup_tab_from_args, DemoTab};

#[test]
fn default_desktop_home_reads_offline_and_keeps_counter_navigation() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(startup_tab_from_args(std::iter::empty())));
    });
    assert!(robot
        .find_by_text("Build native and browser interfaces in Rust.")
        .exists());
    assert!(robot.find_by_text("Get started").click());
    assert!(robot.find_by_text("Create an application").exists());
    assert!(robot.find_by_text("Counter App").click());
    assert!(!robot.find_by_text("Cranpose documentation").exists());
    assert!(robot.find_by_text("Documentation").click());
    assert!(robot
        .find_by_text("Build native and browser interfaces in Rust.")
        .exists());
}

#[test]
fn documentation_is_usable_in_a_compact_window() {
    let mut robot = RobotTestRule::new(390, 780, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Documentation));
    });
    assert!(robot
        .find_by_text("Build native and browser interfaces in Rust.")
        .exists());
    assert!(robot.find_by_text("Get started").click());
    assert!(robot.find_by_text("Create an application").exists());
}

#[test]
fn documentation_aliases_and_existing_startup_routes_are_available() {
    for alias in ["docs", "documentation", "guide"] {
        assert_eq!(
            DemoTab::from_startup_name(alias),
            Some(DemoTab::Documentation)
        );
    }
    assert_eq!(
        DemoTab::from_startup_name("counter"),
        Some(DemoTab::Counter)
    );
}
