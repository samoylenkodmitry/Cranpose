use std::time::Duration;

use cranpose::{AppLauncher, Robot};
use cranpose_testing::{find_button_in_semantics, find_text_by_prefix_in_semantics};
use desktop_app::app;

fn wait_for_prefix(
    robot: &Robot,
    prefix: &str,
    attempts: usize,
    delay: Duration,
) -> Option<(f32, f32, f32, f32, String)> {
    for _ in 0..attempts {
        if let Some(found) = find_text_by_prefix_in_semantics(robot, prefix) {
            return Some(found);
        }
        std::thread::sleep(delay);
    }
    None
}

fn fail(robot: &Robot, message: &str) -> ! {
    eprintln!("✗ {}", message);
    let _ = robot.exit();
    std::process::exit(1);
}

fn dump_semantics(robot: &Robot, label: &str) {
    if let Ok(semantics) = robot.get_semantics() {
        println!("--- Semantics dump ({}) ---", label);
        cranpose::Robot::print_semantics(&semantics, 0);
    }
}

fn click_tab(robot: &Robot, label: &str) {
    let (x, y, w, h) = find_button_in_semantics(robot, label)
        .unwrap_or_else(|| fail(robot, &format!("Tab '{}' not found", label)));
    robot.click(x + w / 2.0, y + h / 2.0).ok();
    std::thread::sleep(Duration::from_millis(200));
    let _ = robot.wait_for_idle();
}

fn parse_pointer_text(text: &str) -> Option<(f32, f32)> {
    let start = text.find('(')? + 1;
    let end = text.find(')')?;
    let coords = &text[start..end];
    let mut parts = coords.split(',');
    let x: f32 = parts.next()?.trim().parse().ok()?;
    let y: f32 = parts.next()?.trim().parse().ok()?;
    Some((x, y))
}

fn main() {
    env_logger::init();
    println!("=== Robot Gradient Tab Switch ===");

    AppLauncher::new()
        .with_title("Gradient Tab Switch Test")
        .with_size(1024, 768)
        .with_headless(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            click_tab(&robot, "CompositionLocal Test");
            if wait_for_prefix(
                &robot,
                "CompositionLocal Subscription Test",
                20,
                Duration::from_millis(100),
            )
            .is_none()
            {
                fail(&robot, "CompositionLocal content not visible");
            }

            click_tab(&robot, "Counter App");
            if wait_for_prefix(
                &robot,
                "Cranpose Playground",
                20,
                Duration::from_millis(100),
            )
            .is_none()
            {
                fail(&robot, "Counter App content not visible");
            }

            let (x, y, w, h, text_before) =
                wait_for_prefix(&robot, "Pointer:", 20, Duration::from_millis(100))
                    .unwrap_or_else(|| fail(&robot, "Pointer text not found"));

            let _ = parse_pointer_text(&text_before)
                .unwrap_or_else(|| fail(&robot, "Failed to parse Pointer text"));

            let pos1 = (x + w * 0.2, y + h * 0.5);
            let pos2 = (x + w * 0.8, y + h * 0.5 + 60.0);

            let _ = robot.mouse_move(pos1.0, pos1.1);
            std::thread::sleep(Duration::from_millis(120));
            let _ = robot.wait_for_idle();

            let (_, _, _, _, text_after_1) =
                wait_for_prefix(&robot, "Pointer:", 10, Duration::from_millis(100)).unwrap_or_else(
                    || {
                        dump_semantics(&robot, "missing pointer after move 1");
                        fail(&robot, "Pointer text missing after first move")
                    },
                );
            let coords_1 = parse_pointer_text(&text_after_1)
                .unwrap_or_else(|| fail(&robot, "Failed to parse Pointer after move 1"));

            let _ = robot.mouse_move(pos2.0, pos2.1);
            std::thread::sleep(Duration::from_millis(120));
            let _ = robot.wait_for_idle();

            let (_, _, _, _, text_after_2) =
                wait_for_prefix(&robot, "Pointer:", 10, Duration::from_millis(100)).unwrap_or_else(
                    || {
                        dump_semantics(&robot, "missing pointer after move 2");
                        fail(&robot, "Pointer text missing after second move")
                    },
                );
            let coords_2 = parse_pointer_text(&text_after_2)
                .unwrap_or_else(|| fail(&robot, "Failed to parse Pointer after move 2"));

            let dx = coords_2.0 - coords_1.0;
            let dy = coords_2.1 - coords_1.1;
            let distance = (dx * dx + dy * dy).sqrt();

            if distance < 5.0 {
                eprintln!(
                    "✗ Pointer coordinates did not update: {:?} -> {:?}",
                    coords_1, coords_2
                );
                let _ = robot.exit();
                std::process::exit(1);
            }

            println!("✓ Pointer coordinates updated after tab switch");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
