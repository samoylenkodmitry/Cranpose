use cranpose_services::{AccessibilityOptions, ProvideAccessibilityOptions};
use cranpose_testing::robot::{RobotTestRule, TestRenderer};
use desktop_app::app::{combined_app_with_initial_tab, startup_tab_from_args, DemoTab};

fn text_bounds(rects: &[(cranpose_ui::Rect, Option<String>)], label: &str) -> cranpose_ui::Rect {
    rects
        .iter()
        .find(|(_, text)| text.as_deref() == Some(label))
        .map(|(bounds, _)| *bounds)
        .expect("text in page snapshot")
}

fn semantic_bounds(root: &cranpose_testing::PlacedSemanticsNode, label: &str) -> cranpose_ui::Rect {
    let mut found = None;
    root.visit(&mut |node| {
        if (found.is_none() || node.clickable) && node.label.as_deref() == Some(label) {
            found = Some(node.target_bounds());
        }
    });
    found.expect("named surface in accessibility tree")
}

fn click_control(robot: &mut RobotTestRule<TestRenderer>, label: &str) -> bool {
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let tree = cranpose_testing::placed_semantics_from_shell(robot.shell_mut())
        .expect("control semantics");
    let (_, height) = robot.viewport_size();
    let mut target = None;
    tree.visit(&mut |node| {
        let bounds = node.target_bounds();
        if node.clickable
            && node.label.as_deref() == Some(label)
            && bounds.y >= 0.0
            && bounds.y + bounds.height <= height as f32
        {
            target = Some(bounds);
        }
    });
    let Some(bounds) = target else {
        return false;
    };
    robot.click_at(
        bounds.x + bounds.width * 0.5,
        bounds.y + bounds.height * 0.5,
    )
}

#[test]
fn reading_scroll_moves_the_tab_row_and_document_together() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Documentation));
    });
    let before = robot.get_all_rects();
    let tabs_before = text_bounds(&before, "Counter App");
    let title_before = text_bounds(&before, "Build native and browser interfaces in Rust.");
    robot.move_to(1040.0, 650.0);
    robot.shell_mut().pointer_scrolled(0.0, -40.0);
    robot.wait_for_idle();
    let after = robot.get_all_rects();
    let tabs_after = text_bounds(&after, "Counter App");
    let title_after = text_bounds(&after, "Build native and browser interfaces in Rust.");
    let displacement = tabs_before.y - tabs_after.y;
    assert!(
        displacement > 20.0,
        "the document must carry its tab row out of view"
    );
    assert!(
        (title_before.y - title_after.y - displacement).abs() < 1.0,
        "the article and tabs must share one scroll offset: tabs {tabs_before:?} -> {tabs_after:?}, title {title_before:?} -> {title_after:?}"
    );
    assert!(click_control(&mut robot, "Get started"));
    let after_jump = robot.get_all_rects();
    let tabs = text_bounds(&after_jump, "Counter App");
    assert!(
        tabs.y + tabs.height <= 0.0,
        "tabs must leave the window after a chapter jump: {tabs:?}"
    );
}

#[test]
fn wheel_brand_rotates_and_a_release_keeps_the_wheel_moving() {
    let mut robot = RobotTestRule::new(390, 780, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Documentation));
    });
    let initial = text_bounds(&robot.get_all_rects(), "The guide.");
    robot.shell_mut().set_cursor(120.0, 550.0);
    robot.shell_mut().pointer_pressed_at_time(Some(0));
    for step in 1..=8 {
        robot.advance_time(16_666_667);
        robot
            .shell_mut()
            .set_cursor_at_time(120.0, 550.0 - step as f32 * 12.0, Some(step * 16));
    }
    let held = text_bounds(&robot.get_all_rects(), "The guide.");
    assert!(
        held.y < initial.y - 30.0,
        "the brand belongs to the rotating wheel"
    );
    robot
        .shell_mut()
        .pointer_released_at_position_time(120.0, 454.0, Some(136));
    for _ in 0..30 {
        robot.advance_time(16_666_667);
    }
    let released = text_bounds(&robot.get_all_rects(), "The guide.");
    assert!(
        released.y < held.y - 30.0,
        "the wheel must coast after a fast release: {held:?} -> {released:?}"
    );
}

#[test]
fn wide_article_aligns_with_the_wheel_edge() {
    let mut robot = RobotTestRule::new(1800, 1000, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Documentation));
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let tree =
        cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("reader semantics");
    let glass = semantic_bounds(&tree, "Documentation glass");
    let text = text_bounds(
        &robot.get_all_rects(),
        "Build native and browser interfaces in Rust.",
    );
    assert!(text.x - glass.x <= 41.0, "article should align with the wheel, not the center of the remaining window: {text:?}, {glass:?}");
}

#[test]
fn wheel_brand_clears_the_tabs_in_a_short_window() {
    let mut robot = RobotTestRule::new(800, 600, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Documentation));
    });
    let rects = robot.get_all_rects();
    let brand = text_bounds(&rects, "CRANPOSE");
    let tabs = text_bounds(&rects, "Counter App");
    assert!(
        brand.y > tabs.y + tabs.height + 12.0,
        "brand must clear the initial tabs: {brand:?}, {tabs:?}"
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
    assert!(click_control(&mut robot, "Get started"));
    assert!(robot.find_by_text("Create an application").exists());
    robot.move_to(1040.0, 650.0);
    robot.shell_mut().pointer_scrolled(0.0, 9000.0);
    robot.wait_for_idle();
    assert!(click_control(&mut robot, "Counter App"));
    assert!(!robot.find_by_text("Cranpose documentation").exists());
    assert!(click_control(&mut robot, "Documentation"));
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
    assert!(wheel.width <= glass.x && wheel.height > 800.0);
    let x = glass.x + glass.width - 20.0;
    let y = 650.0;
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
    assert!(!robot
        .find_by_text("Build native and browser interfaces in Rust.")
        .exists());
    assert!(robot.find_by_text("Welcome").exists());
    assert!(robot.find_by_text("View on GitHub").exists());
    robot.move_to(150.0, 620.0);
    robot.shell_mut().pointer_scrolled(0.0, -150.0);
    robot.wait_for_idle();
    assert!(!robot.find_by_text("Create an application").exists());
    assert!(click_control(&mut robot, "Get started"));
    assert!(robot.find_by_text("Create an application").exists());
    assert!(click_control(&mut robot, "Back to wheel"));
    assert!(!robot.find_by_text("Create an application").exists());
    assert!(click_control(&mut robot, "Get started"));
    assert!(robot.find_by_text("Create an application").exists());
}

#[test]
fn wheel_scroll_navigates_the_article_and_github_stays_at_the_window_corner() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Documentation));
    });
    let before = robot.get_all_rects();
    let github = before
        .iter()
        .find(|(_, label)| label.as_deref() == Some("View on GitHub"))
        .map(|(bounds, _)| *bounds)
        .expect("repository label");
    assert!(github.x < 100.0 && github.y > 740.0);
    robot.move_to(160.0, 620.0);
    robot.shell_mut().pointer_scrolled(0.0, -150.0);
    robot.wait_for_idle();
    let after = robot.get_all_rects();
    let anchored = after
        .iter()
        .find(|(_, label)| label.as_deref() == Some("View on GitHub"))
        .map(|(bounds, _)| *bounds)
        .expect("repository label");
    assert!((github.y - anchored.y).abs() < 1.0);
    assert!(click_control(&mut robot, "Get started"));
    let after = robot.get_all_rects();
    let article = text_bounds(&after, "Create an application");
    assert!(
        article.y >= 0.0 && article.y < 600.0,
        "wheel must bring the next section into view: {article:?}"
    );
}

#[test]
fn reader_actions_move_between_chapters_without_the_section_wheel() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Documentation));
    });
    assert!(robot.find_by_text("View on GitHub").exists());
    assert!(click_control(&mut robot, "Next section"));
    assert!(robot.find_by_text("Create an application").exists());
    assert!(click_control(&mut robot, "Previous section"));
    assert!(robot
        .find_by_text("Build native and browser interfaces in Rust.")
        .exists());
}

#[test]
fn selected_chapter_survives_resizing_between_reader_layouts() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Documentation));
    });
    assert!(click_control(&mut robot, "Get started"));
    robot.set_viewport(390, 780);
    assert!(robot.find_by_text("Create an application").exists());
    assert!(click_control(&mut robot, "Back to wheel"));
    assert!(click_control(&mut robot, "Get started"));
    assert!(robot.find_by_text("Create an application").exists());
    robot.set_viewport(1200, 800);
    assert!(robot.find_by_text("Create an application").exists());
    assert!(robot.find_by_text("View on GitHub").exists());
}

#[test]
fn compact_wheel_reveals_the_current_chapter_after_reading_to_the_end() {
    let mut robot = RobotTestRule::new(390, 780, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Documentation));
    });
    assert!(click_control(&mut robot, "Welcome"));
    for _ in 0..9 {
        assert!(click_control(&mut robot, "Next section"));
    }
    assert!(robot
        .find_by_text("0.9 is the stabilization release line.")
        .exists());
    assert!(click_control(&mut robot, "Back to wheel"));
    let current = robot
        .find_by_text("Road to 1.0")
        .bounds()
        .expect("current chapter");
    assert!(current.y >= 0.0 && current.y + current.height <= 780.0);
    assert!(click_control(&mut robot, "Road to 1.0"));
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
    assert!(click_control(&mut robot, "Welcome"));
    let action = robot
        .find_by_text("Next section")
        .bounds()
        .expect("next chapter action");
    assert!(action.x >= 0.0 && action.x + action.width <= 390.0);
    assert!(action.y >= 0.0 && action.y + action.height <= 780.0);
    assert!(click_control(&mut robot, "Next section"));
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

#[test]
fn article_scroll_rotates_the_wheel_to_the_current_section() {
    let mut robot = RobotTestRule::new(1200, 800, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Documentation));
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    let tree =
        cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("wheel semantics");
    let welcome_before = semantic_bounds(&tree, "Get started");
    robot.move_to(1040.0, 650.0);
    for _ in 0..20 {
        robot.shell_mut().pointer_scrolled(0.0, -180.0);
        robot.wait_for_idle();
        let tree =
            cranpose_testing::placed_semantics_from_shell(robot.shell_mut()).expect("semantics");
        let mut current = false;
        tree.visit(&mut |node| {
            if node.label.as_deref() == Some("Get started") && node.selected == Some(true) {
                current = true;
            }
        });
        if current {
            let welcome_after = semantic_bounds(&tree, "Get started");
            assert!(welcome_after.y < welcome_before.y - 70.0);
            return;
        }
    }
    panic!("reading into Get started must select and rotate its wheel entry");
}
