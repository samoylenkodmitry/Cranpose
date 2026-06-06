//! Robot regression: after visiting animated tabs and settling on static content,
//! the app must not keep presenting frames or mutating FPS stats.

use cranpose::AppLauncher;
use cranpose_testing::find_button_exact_in_semantics;
use desktop_app::app;
use std::time::Duration;

const WINDOW_TITLE: &str = "Robot Idle FPS After Tab Walk";

fn main() {
    env_logger::init();
    println!("=== Robot Idle FPS After Tab Walk ===");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(1080, 820)
        .with_headless(false)
        .with_fps_counter(true)
        .with_frame_pacing_controls(true)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(700));
            let _ = robot.wait_for_idle();

            for tab in [
                "Winamp",
                "Shaders",
                "Shader Rect",
                "Markdown",
                "Interactive Anim",
            ] {
                click_tab(&robot, tab);
            }
            click_tab(&robot, "Text");

            robot
                .wait_for_idle()
                .expect("static Text tab should settle after click-path tab walk");
            robot.reset_fps_stats().expect("reset idle FPS stats");
            log_runtime_stats(&robot, "after-reset");
            std::thread::sleep(Duration::from_millis(600));
            log_runtime_stats(&robot, "after-sleep");
            let stats_before_idle_wait =
                robot.fps_stats().expect("read FPS stats before idle wait");
            println!("fps after sleep before idle wait: {stats_before_idle_wait:?}");
            robot.wait_for_idle().expect("idle wait should remain idle");
            log_runtime_stats(&robot, "after-idle-wait");
            let stats = robot.fps_stats().expect("read idle FPS stats");

            assert_eq!(
                stats.frame_count, 0,
                "idle static tab presented frames after settling: {stats:?}"
            );

            println!("PASS: idle static tab produced no presented frames");
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn click_tab(robot: &cranpose::Robot, label: &str) {
    let Some((x, y, width, height)) = wait_for_tab(robot, label) else {
        panic!("tab button {label:?} was not found");
    };
    robot
        .click(x + width * 0.5, y + height * 0.5)
        .unwrap_or_else(|err| panic!("failed to click tab {label:?}: {err}"));
    std::thread::sleep(Duration::from_millis(220));
}

fn wait_for_tab(robot: &cranpose::Robot, label: &str) -> Option<(f32, f32, f32, f32)> {
    for _ in 0..30 {
        if let Some(bounds) = find_button_exact_in_semantics(robot, label) {
            return Some(bounds);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

fn log_runtime_stats(robot: &cranpose::Robot, phase: &str) {
    match robot.get_runtime_leak_debug_stats() {
        Ok(stats) => {
            println!(
                "runtime {phase}: frame_callbacks={} tasks={} invalid_scopes={} scope_queue={} node_updates={} ui_pending={}",
                stats.runtime_stats.frame_callbacks_len,
                stats.runtime_stats.tasks_len,
                stats.runtime_stats.invalid_scopes_len,
                stats.runtime_stats.scope_queue_len,
                stats.runtime_stats.node_updates_len,
                stats.runtime_stats.ui_dispatcher_pending
            );
        }
        Err(err) => {
            println!("runtime {phase}: unavailable: {err}");
        }
    }
}
