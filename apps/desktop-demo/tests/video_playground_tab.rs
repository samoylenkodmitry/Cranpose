// Integration tests for the Video Playground showcase tab driven through public
// entry points: stress scenarios (including the 64-video wall), animated surface
// effect toggles, per-source license catalog audit, and clean disposal on tab exit.

use cranpose_testing::{
    placed_semantics_from_shell,
    robot::{RobotTestRule, TestRenderer},
    PlacedSemanticsNode,
};
use desktop_app::app::{combined_app_with_initial_tab, DemoTab};

use crate::tab_switch_regression_support::{pump_shell_until_stable, set_active_tab};

fn click_button(robot: &mut RobotTestRule<TestRenderer>, label: &str) {
    assert!(
        robot.find_by_text(label).click(),
        "expected clickable control {label:?}"
    );
    pump_shell_until_stable(robot.shell_mut());
}

fn shows(robot: &mut RobotTestRule<TestRenderer>, needle: &str) -> bool {
    robot
        .get_all_text()
        .iter()
        .any(|text| text.contains(needle))
}

fn semantics_contains_description(node: &PlacedSemanticsNode, target: &str) -> bool {
    if node.label.as_deref() == Some(target) {
        return true;
    }
    node.children
        .iter()
        .any(|child| semantics_contains_description(child, target))
}

#[test]
fn startup_slug_and_aliases_select_video_playground() {
    for name in ["video", "Video", "videoplayground", "videowall", "player"] {
        assert_eq!(
            DemoTab::from_startup_name(name),
            Some(DemoTab::VideoPlayground),
            "startup name {name:?} should select Video Playground"
        );
    }
}

#[test]
fn stress_scenarios_effects_and_64_video_wall_render_and_dispose_cleanly() {
    let mut robot = RobotTestRule::new(1440, 960, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::VideoPlayground));
    });
    robot.shell_mut().set_semantics_enabled(true);
    pump_shell_until_stable(robot.shell_mut());

    // Default scenario: 4x 4K 60 with all 6 independent telemetry headings visible.
    for expected in [
        "Video Playground",
        "1.99B px/s",
        "Decoder FPS",
        "Compositor FPS",
        "Dropped Frames",
        "GPU Video / RSS",
        "CPU Utilization",
        "Frame-Copy Overhead",
    ] {
        assert!(
            shows(&mut robot, expected),
            "expected telemetry/header text {expected:?}"
        );
    }

    // Switch through stress scenarios up to the 64-video wall (3.54B px/s).
    click_button(&mut robot, "1x 4K 120");
    assert!(shows(&mut robot, "995M px/s"));

    click_button(&mut robot, "1x 8K 60");
    assert!(shows(&mut robot, "1x 8K 60 · 1.99B px/s"));

    click_button(&mut robot, "64x 720p Wall");
    assert!(shows(&mut robot, "3.54B px/s"));
    let placed = placed_semantics_from_shell(robot.shell_mut())
        .expect("semantics tree should exist for 64-video wall");
    assert!(
        semantics_contains_description(&placed, "Video tile 64"),
        "64-video wall must mount tile #64"
    );

    // Exercise animated layouts and GPU surface effects (blur, alpha, WGSL shader).
    click_button(&mut robot, "Layout: Wave");
    click_button(&mut robot, "Blur: Off");
    assert!(shows(&mut robot, "Blur: On"));
    click_button(&mut robot, "Alpha: Off");
    assert!(shows(&mut robot, "Alpha: On"));
    click_button(&mut robot, "Shader: Off");
    assert!(shows(&mut robot, "Shader: Prism"));
    click_button(&mut robot, "Layout: Spotlight");

    // Open curated media catalog and verify license disclosures (including UVG non-commercial).
    click_button(&mut robot, "Open Media Catalog");
    for license_text in [
        "Sol Levante",
        "Nocturne",
        "Tears of Steel",
        "Charge",
        "Ultra Video Group",
        "CC BY-NC 4.0 (Non-Commercial ONLY)",
        "Jellyfin 4K HDR10 HEVC",
        "CC BY-SA 4.0",
    ] {
        assert!(
            shows(&mut robot, license_text),
            "expected media catalog disclosure {license_text:?}"
        );
    }

    // Switching away must release all decoders and render the target static tab cleanly.
    set_active_tab(DemoTab::Text);
    pump_shell_until_stable(robot.shell_mut());
    assert!(!shows(&mut robot, "Frame-Copy Overhead"));
}
