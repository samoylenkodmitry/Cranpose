use cranpose_services::{AccessibilityOptions, ProvideAccessibilityOptions};
use cranpose_testing::robot::{RobotTestRule, TestRenderer};
use desktop_app::app::{combined_app_with_initial_tab, startup_tab_from_args, DemoTab};

fn text_bounds(rects: &[(cranpose_ui::Rect, Option<String>)], label: &str) -> cranpose_ui::Rect {
    rects
        .iter()
        .find(|(_, text)| text.as_ref().is_some_and(|text| text.contains(label)))
        .map(|(bounds, _)| *bounds)
        .expect("text in page snapshot")
}

fn semantic_bounds(root: &cranpose_testing::PlacedSemanticsNode, label: &str) -> cranpose_ui::Rect {
    let mut found = None;
    root.visit(&mut |node| {
        if node.label.as_deref() == Some(label) {
            found = Some(node.target_bounds());
        }
    });
    found.expect("named surface in accessibility tree")
}

#[test]
fn reading_scroll_moves_the_tab_row_and_document_together() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Documentation));
    });
    let before = robot.get_all_rects();
    let tabs_before = text_bounds(&before, "Counter App");
    let title_before = text_bounds(&before, "Build native and browser interfaces in Rust.");
    robot.drag(1040.0, 650.0, 1040.0, 300.0);
    let after = robot.get_all_rects();
    let tabs_after = text_bounds(&after, "Counter App");
    let title_after = text_bounds(&after, "Build native and browser interfaces in Rust.");
    let displacement = tabs_before.y - tabs_after.y;
    assert!(
        displacement > 100.0,
        "the document must carry its tab row out of view"
    );
    assert!(
        (title_before.y - title_after.y - displacement).abs() < 1.0,
        "the article and tabs must share one scroll offset: tabs {tabs_before:?} -> {tabs_after:?}, title {title_before:?} -> {title_after:?}"
    );
}

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
fn wheel_extends_beneath_the_edge_to_edge_reader_without_stealing_clicks() {
    let mut robot = RobotTestRule::new(1200, 1400, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Documentation));
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let tree =
        cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("placed semantics");
    let page = semantic_bounds(&tree, "Cranpose documentation");
    let glass = semantic_bounds(&tree, "Documentation glass");
    let wheel = semantic_bounds(&tree, "Documentation wheel");
    assert_eq!(page.x, 0.0);
    assert_eq!(page.width, 1200.0);
    assert_eq!(glass.x + glass.width, 1200.0);
    assert!(wheel.width > glass.width && wheel.height > 1000.0);
    let covered = semantic_bounds(&tree, "Text and accessibility");
    let x = covered.x + covered.width * 0.5;
    let y = covered.y + covered.height * 0.5;
    assert!(x > glass.x && y < 1400.0);
    robot.click_at(x, y);
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
    assert!(robot.find_by_text("Next section").click());
    assert!(robot.find_by_text("Create an application").exists());
    assert!(robot.find_by_text("Previous section").click());
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
        assert!(robot.find_by_text("Next section").click());
    }
    assert!(!robot.find_by_text("Next section").exists());
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
        .find_by_text("Next section")
        .bounds()
        .expect("next chapter action");
    assert!(action.x >= 0.0 && action.x + action.width <= 390.0);
    assert!(action.y >= 0.0 && action.y + action.height <= 780.0);
    assert!(robot.find_by_text("Next section").click());
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
