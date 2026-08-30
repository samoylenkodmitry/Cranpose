mod robot_launch;

mod regression_robot_support;

use std::time::Duration;

use desktop_app::app;
use regression_robot_support::{
    click_button, semantics_dump, spawn_timeout, wait_for_text, wait_for_text_prefix,
};

fn main() {
    env_logger::init();
    println!("=== Robot Recursive Layout Depth Six Test ===");

    robot_launch::launch("Robot Recursive Layout Depth Six Test", 1024, 768)
        .with_test_driver(|robot| {
            spawn_timeout(30, "robot_recursive_layout_depth_six");

            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            click_button(&robot, "Recursive Layout")
                .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(&robot)));
            wait_for_text(
                &robot,
                "Recursive Layout Playground",
                30,
                Duration::from_millis(100),
            )
            .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(&robot)));

            let (_, _, _, _, initial_depth) =
                wait_for_text_prefix(&robot, "Current depth:", 20, Duration::from_millis(100))
                    .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(&robot)));
            println!("Initial recursive layout depth: {initial_depth}");

            for depth in 4..=6 {
                click_button(&robot, "Increase depth")
                    .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(&robot)));

                let (_, _, _, _, current_depth) =
                    wait_for_text_prefix(&robot, "Current depth:", 30, Duration::from_millis(100))
                        .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(&robot)));
                assert_eq!(
                    current_depth,
                    format!("Current depth: {depth}"),
                    "recursive layout depth label stalled while increasing to {depth}\n{}",
                    semantics_dump(&robot)
                );

                wait_for_text(
                    &robot,
                    &format!("Depth {depth}"),
                    30,
                    Duration::from_millis(100),
                )
                .unwrap_or_else(|err| panic!("{err}\n{}", semantics_dump(&robot)));
            }

            println!("✓ Recursive Layout reached depth 6 without crashing");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
