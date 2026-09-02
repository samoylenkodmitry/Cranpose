mod robot_exit;
mod robot_launch;

use std::time::Duration;

use cranpose::{Robot, RobotScreenshot};
use cranpose_testing::capture_screenshot;
use desktop_app::test_screens::nested_glass_cache_repro::{
    shader_phase_color, NestedGlassCacheReproScreen, BACKGROUND_TOGGLE_RECT, NESTED_BUTTON_RECT,
    SCREEN_HEIGHT, SCREEN_WIDTH, SHADER_BOX_RECT, SHADER_TOGGLE_RECT, TICK_RECT,
};

const WARMUP_TICKS: u32 = 4;
const STILL_TICKS: u32 = 6;
const COLOR_TOLERANCE: i32 = 12;
const MAX_BACKGROUND_TOGGLE_PASSES: u32 = 13;
const MAX_SHADER_TOGGLE_PASSES: u32 = 10;

fn center(rect: [f32; 4]) -> (f32, f32) {
    (rect[0] + rect[2] * 0.5, rect[1] + rect[3] * 0.5)
}

fn pixel_at_logical(screenshot: &RobotScreenshot, x: f32, y: f32) -> [u8; 4] {
    let scale = if screenshot.logical_width.is_finite() && screenshot.logical_width > 0.0 {
        screenshot.width as f32 / screenshot.logical_width
    } else {
        1.0
    };
    let px = ((x * scale) as u32).min(screenshot.width.saturating_sub(1));
    let py = ((y * scale) as u32).min(screenshot.height.saturating_sub(1));
    let index = (py as usize * screenshot.width as usize + px as usize) * 4;
    let bytes = &screenshot.pixels[index..index + 4];
    [bytes[0], bytes[1], bytes[2], bytes[3]]
}

fn color_close(actual: [u8; 4], expected: [u8; 4]) -> bool {
    actual
        .iter()
        .zip(expected.iter())
        .take(3)
        .all(|(a, e)| (*a as i32 - *e as i32).abs() <= COLOR_TOLERANCE)
}

fn click_rect(robot: &Robot, rect: [f32; 4]) {
    let (x, y) = center(rect);
    robot.click(x, y).expect("click");
    robot.wait_for_idle().expect("idle");
}

fn screenshot(robot: &Robot) -> RobotScreenshot {
    capture_screenshot(robot).unwrap_or_else(|| robot_exit::fail(robot, "screenshot failed"))
}

struct FrameCounts {
    isolated_layer_renders: u32,
    layer_cache_misses: u32,
    pass_count: u32,
}

fn render_frame(robot: &Robot) -> (RobotScreenshot, FrameCounts) {
    let screenshot = screenshot(robot);
    let stats = robot
        .get_render_stats()
        .unwrap_or_else(|err| robot_exit::fail(robot, &format!("render stats: {err}")))
        .unwrap_or_else(|| robot_exit::fail(robot, "renderer published no frame stats"));
    (
        screenshot,
        FrameCounts {
            isolated_layer_renders: stats.isolated_layer_renders,
            layer_cache_misses: stats.layer_cache_misses,
            pass_count: stats.pass_count,
        },
    )
}

fn expect_still_frames(robot: &Robot, stage: &str) {
    for tick in 0..STILL_TICKS {
        click_rect(robot, TICK_RECT);
        let (_, counts) = render_frame(robot);
        println!(
            "{stage}: tick {tick} isolated_layer_renders={} layer_cache_misses={}",
            counts.isolated_layer_renders, counts.layer_cache_misses
        );
        if counts.isolated_layer_renders != 0 {
            robot_exit::fail(
                robot,
                &format!(
                    "{stage}: a frame that only changed the tick strip re-rendered {} isolated layer(s); \
                     the glass card with a runtime-shader child and a nested glass button must be served from the layer cache",
                    counts.isolated_layer_renders
                ),
            );
        }
    }
}

fn expect_card_rerender(robot: &Robot, stage: &str, max_passes: u32) -> RobotScreenshot {
    let (screenshot, counts) = render_frame(robot);
    println!(
        "{stage}: isolated_layer_renders={} layer_cache_misses={} passes={}",
        counts.isolated_layer_renders, counts.layer_cache_misses, counts.pass_count
    );
    if counts.isolated_layer_renders == 0 {
        robot_exit::fail(
            robot,
            &format!("{stage}: the card content changed but no isolated layer was re-rendered"),
        );
    }
    if counts.pass_count > max_passes {
        robot_exit::fail(
            robot,
            &format!(
                "{stage}: re-rendering the card took {} render passes, more than the {max_passes} a baked underlay allows; \
                 the underlay under a plainly composited card and the nested button's capture must be texture copies, not passes",
                counts.pass_count
            ),
        );
    }
    screenshot
}

fn main() {
    env_logger::init();
    println!("=== Robot Nested Glass Cache Test ===");
    robot_launch::launch(
        "Robot Nested Glass Cache",
        SCREEN_WIDTH as u32,
        SCREEN_HEIGHT as u32,
    )
    .with_test_driver(|robot| {
        std::thread::sleep(Duration::from_millis(300));
        let _ = robot.wait_for_idle();
        for _ in 0..WARMUP_TICKS {
            click_rect(&robot, TICK_RECT);
            let _ = render_frame(&robot);
        }

        expect_still_frames(&robot, "still-a");
        let before_background = screenshot(&robot);
        let (button_x, button_y) = center(NESTED_BUTTON_RECT);
        let button_before = pixel_at_logical(&before_background, button_x, button_y);

        click_rect(&robot, BACKGROUND_TOGGLE_RECT);
        let after_background = expect_card_rerender(&robot, "background-toggle", MAX_BACKGROUND_TOGGLE_PASSES);
        let button_after = pixel_at_logical(&after_background, button_x, button_y);
        if color_close(button_before, button_after) {
            robot_exit::fail(
                &robot,
                &format!(
                    "the nested glass button kept its stale backdrop after the background changed: {button_before:?} -> {button_after:?}"
                ),
            );
        }
        println!("nested button refreshed with the backdrop: {button_before:?} -> {button_after:?}");

        expect_still_frames(&robot, "still-b");

        let (shader_x, shader_y) = center(SHADER_BOX_RECT);
        let shader_before = pixel_at_logical(&screenshot(&robot), shader_x, shader_y);
        if !color_close(shader_before, shader_phase_color(0.2)) {
            robot_exit::fail(
                &robot,
                &format!(
                    "runtime shader child should show phase 0.2 before the toggle, got {shader_before:?}"
                ),
            );
        }
        click_rect(&robot, SHADER_TOGGLE_RECT);
        let after_shader = expect_card_rerender(&robot, "shader-toggle", MAX_SHADER_TOGGLE_PASSES);
        let shader_after = pixel_at_logical(&after_shader, shader_x, shader_y);
        if !color_close(shader_after, shader_phase_color(0.8)) {
            robot_exit::fail(
                &robot,
                &format!(
                    "runtime shader child must repaint when its uniforms change, expected phase 0.8 got {shader_after:?}"
                ),
            );
        }
        println!("runtime shader child repainted on uniform change: {shader_before:?} -> {shader_after:?}");

        expect_still_frames(&robot, "still-c");

        println!("✓ PASS: nested glass card is cached on still frames and refreshed on real changes");
        let _ = robot.exit();
    })
    .run(|| {
        NestedGlassCacheReproScreen();
    });
}
