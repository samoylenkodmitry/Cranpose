mod robot_exit;
mod robot_launch;

use std::time::Duration;

use cranpose::{Robot, RobotScreenshot};
use desktop_app::test_screens::nested_glass_cache_repro::{
    NestedGlassAnimatedReproScreen, SCREEN_HEIGHT, SCREEN_WIDTH, SHADER_BOX_RECT,
};

const SAMPLES: usize = 6;
const FRAMES_BETWEEN_SAMPLES: u32 = 12;
const MIN_CHANNEL_DELTA: i32 = 8;

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

fn shader_pixel(robot: &Robot) -> [u8; 4] {
    let shot = robot
        .screenshot()
        .unwrap_or_else(|err| robot_exit::fail(robot, &format!("screenshot: {err}")));
    let (x, y) = (
        SHADER_BOX_RECT[0] + SHADER_BOX_RECT[2] * 0.5,
        SHADER_BOX_RECT[1] + SHADER_BOX_RECT[3] * 0.5,
    );
    pixel_at_logical(&shot, x, y)
}

fn main() {
    env_logger::init();
    println!("=== Robot Nested Glass Animated Shader Test ===");
    robot_launch::launch(
        "Robot Nested Glass Animated Shader",
        SCREEN_WIDTH as u32,
        SCREEN_HEIGHT as u32,
    )
    .with_test_driver(|robot| {
        std::thread::sleep(Duration::from_millis(300));
        robot.pump_frames(4).expect("settle");
        let mut previous = shader_pixel(&robot);
        for sample in 0..SAMPLES {
            std::thread::sleep(Duration::from_millis(120));
            robot.pump_frames(FRAMES_BETWEEN_SAMPLES).expect("advance animation");
            let current = shader_pixel(&robot);
            let moved = previous
                .iter()
                .zip(current.iter())
                .take(3)
                .any(|(a, b)| (*a as i32 - *b as i32).abs() >= MIN_CHANNEL_DELTA);
            println!("sample {sample}: {previous:?} -> {current:?} moved={moved}");
            if !moved {
                robot_exit::fail(
                    &robot,
                    &format!(
                        "the runtime shader inside a cached glass card froze: its uniforms advance on the frame clock but the card kept serving a stale surface ({previous:?} -> {current:?})"
                    ),
                );
            }
            previous = current;
        }
        println!("✓ PASS: frame-clock uniforms keep repainting through the cached card");
        let _ = robot.exit();
    })
    .run(|| {
        NestedGlassAnimatedReproScreen();
    });
}
