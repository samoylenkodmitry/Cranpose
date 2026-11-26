// Integration tests for the Video Playground showcase tab driven through public
// entry points: Big Buck Bunny default source, motion disabled by default, play/pause/seek
// transport controls, stress scenarios up to the 64-video wall, TikTok-style Reels gallery,
// per-source license catalog audit (including 8K nature & UVG non-commercial), and clean disposal.

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
fn defaults_transport_stress_wall_reels_and_catalog_work_cleanly() {
    let mut robot = RobotTestRule::new(1440, 960, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::VideoPlayground));
    });
    robot.shell_mut().set_semantics_enabled(true);
    pump_shell_until_stable(robot.shell_mut());

    // Default state: Big Buck Bunny open by default, motion & 3D off by default, full transport
    // controls (Play/Pause, -10s, +10s, seek bar), and all 6 independent telemetry headings.
    for expected in [
        "Video Playground",
        "Big Buck Bunny (1080p 60 FPS)",
        "Motion: Off",
        "3D: Off",
        "-10s",
        "+10s",
        "Decoder FPS",
        "Compositor FPS",
        "Dropped Frames",
        "GPU Video / RSS",
        "CPU Utilization",
        "Frame-Copy Overhead",
    ] {
        assert!(
            shows(&mut robot, expected),
            "expected default Video Playground text {expected:?}"
        );
    }
    let initial_placed = placed_semantics_from_shell(robot.shell_mut())
        .expect("semantics tree should exist on Video Playground");
    assert!(
        semantics_contains_description(&initial_placed, "Video seek bar"),
        "interactive seek bar must expose accessible semantics"
    );

    // Exercise play/pause and seek controls without panicking.
    click_button(&mut robot, "+10s");
    click_button(&mut robot, "-10s");
    click_button(&mut robot, "Speed: 1x");
    assert!(shows(&mut robot, "Speed: 1.5x"));

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

    // Exercise animated layouts and GPU surface effects (motion, blur, alpha, WGSL shader).
    click_button(&mut robot, "Motion: Off");
    assert!(shows(&mut robot, "Motion: On"));
    click_button(&mut robot, "Layout: Wave");
    click_button(&mut robot, "Blur: Off");
    assert!(shows(&mut robot, "Blur: On"));
    click_button(&mut robot, "Alpha: Off");
    assert!(shows(&mut robot, "Alpha: On"));
    click_button(&mut robot, "Shader: Off");
    assert!(shows(&mut robot, "Shader: Prism"));
    click_button(&mut robot, "Layout: Spotlight");

    // Enter TikTok-style scrollable vertical Reels gallery and navigate to the next reel.
    click_button(&mut robot, "Layout: Reels");
    assert!(shows(&mut robot, "Reel 1 /"));
    click_button(&mut robot, "Next");
    assert!(shows(&mut robot, "Reel 2 /"));
    click_button(&mut robot, "Video Wall");

    // Open curated media catalog and verify direct streams (Sol Levante, Charge, 8K Nature,
    // Jellyfin ams1) plus license disclosures (including UVG non-commercial).
    click_button(&mut robot, "Open Media Catalog");
    for expected in [
        "Play Big Buck Bunny (MPEG-DASH ABR)",
        "Play Charge 4K (Blender Open Movie)",
        "Play Sol Levante 4K (Netflix Open Content)",
        "Play Potomac River Rapids — 8K Nature",
        "Play Earth from the ISS — NASA 8K Ultra HD",
        "Play Jellyfin 4K HDR10 HEVC (40 Mbps)",
        "Ultra Video Group",
        "CC BY-NC 4.0 (Non-Commercial ONLY)",
        "CC BY-SA 4.0",
    ] {
        assert!(
            shows(&mut robot, expected),
            "expected media catalog entry/disclosure {expected:?}"
        );
    }

    // Switching away must release all decoders and render the target static tab cleanly.
    set_active_tab(DemoTab::Text);
    pump_shell_until_stable(robot.shell_mut());
    assert!(!shows(&mut robot, "Frame-Copy Overhead"));
}
