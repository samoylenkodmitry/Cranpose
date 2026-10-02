use cranpose_services::{AccessibilityOptions, ProvideAccessibilityOptions};
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
    assert!(robot.find_by_text("Sections").click());
    assert!(robot.find_by_text("View on GitHub").exists());
    assert!(robot.find_by_text("Get started").click());
    assert!(robot.find_by_text("Create an application").exists());
    assert!(robot.find_by_text("Sections").exists());
    assert!(!robot.find_by_text("Close sections").exists());
}

#[test]
fn reader_actions_move_between_chapters_without_the_section_wheel() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Documentation));
    });
    assert!(robot.find_by_text("View on GitHub").exists());
    assert!(robot.find_by_text("Next section →").click());
    assert!(robot.find_by_text("Create an application").exists());
    assert!(robot.find_by_text("← Previous").click());
    assert!(robot
        .find_by_text("Build native and browser interfaces in Rust.")
        .exists());
}

#[test]
fn selected_chapter_survives_resizing_between_reader_layouts() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Documentation));
    });
    assert!(robot.find_by_text("Get started").click());
    robot.set_viewport(390, 780);
    assert!(robot.find_by_text("Create an application").exists());
    assert!(robot.find_by_text("Sections").click());
    assert!(robot.find_by_text("Close sections").click());
    assert!(robot.find_by_text("Create an application").exists());
    robot.set_viewport(1200, 800);
    assert!(robot.find_by_text("Create an application").exists());
    assert!(robot.find_by_text("View on GitHub").exists());
}

#[test]
fn compact_sections_reveal_the_current_chapter_after_reading_to_the_end() {
    let mut robot = RobotTestRule::new(390, 780, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Documentation));
    });
    for _ in 0..9 {
        assert!(robot.find_by_text("Next section →").click());
    }
    assert!(!robot.find_by_text("Next section →").exists());
    assert!(robot.find_by_text("Sections").click());
    let current = robot
        .find_by_text("Road to 1.0")
        .bounds()
        .expect("current chapter");
    assert!(current.y >= 0.0 && current.y + current.height <= 780.0);
    assert!(robot.find_by_text("Road to 1.0").click());
    assert!(robot
        .find_by_text("0.9 is the stabilization release line.")
        .exists());
}

#[test]
fn reader_navigation_fits_with_larger_text_and_reduced_effects() {
    let mut robot = RobotTestRule::new(390, 780, TestRenderer::default(), || {
        ProvideAccessibilityOptions(
            AccessibilityOptions {
                font_scale: 1.5,
                reduce_motion: true,
                reduce_transparency: true,
                ..Default::default()
            },
            || combined_app_with_initial_tab(Some(DemoTab::Documentation)),
        );
    });
    let action = robot
        .find_by_text("Next section →")
        .bounds()
        .expect("next chapter action");
    assert!(action.x >= 0.0 && action.x + action.width <= 390.0);
    assert!(action.y >= 0.0 && action.y + action.height <= 780.0);
    assert!(robot.find_by_text("Next section →").click());
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
