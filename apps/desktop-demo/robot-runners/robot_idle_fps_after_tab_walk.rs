//! Robot regression: after visiting animated tabs and settling on static content,
//! the app must not keep presenting frames or mutating FPS stats.

mod perf_contract;

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

            let hardware_perf = perf_contract::hardware_performance_contracts_enabled();
            if hardware_perf {
                assert_eq!(
                    stats.frame_count, 0,
                    "idle static tab presented frames after settling: {stats:?}"
                );
            } else {
                assert_idle_runtime_queues_empty(&robot);
                perf_contract::log_software_renderer_budget(
                    "idle static tab presented-frame quiescence",
                );
            }

            if hardware_perf {
                println!("PASS: idle static tab produced no presented frames");
            } else {
                println!("PASS: idle static tab left no runtime work queued");
            }
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn assert_idle_runtime_queues_empty(robot: &cranpose::Robot) {
    let stats = robot
        .get_runtime_leak_debug_stats()
        .expect("idle runtime stats should be available");
    assert_eq!(
        stats.runtime_stats.frame_callbacks_len, 0,
        "idle static tab left frame callbacks queued: {stats:?}"
    );
    assert_eq!(
        stats.runtime_stats.tasks_len,
        0,
        "idle static tab left runtime tasks queued: {:?}\nstats: {stats:?}",
        robot.live_ui_task_labels().unwrap_or_default()
    );
    assert_eq!(
        stats.runtime_stats.invalid_scopes_len, 0,
        "idle static tab left invalid scopes queued: {stats:?}"
    );
    assert_eq!(
        stats.runtime_stats.node_updates_len, 0,
        "idle static tab left node updates queued: {stats:?}"
    );
    assert_eq!(
        stats.runtime_stats.ui_dispatcher_pending, 0,
        "idle static tab left UI dispatcher tasks pending: {stats:?}"
    );
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
