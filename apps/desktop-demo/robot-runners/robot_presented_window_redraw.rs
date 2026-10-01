use crate::{output_paths, text_showcase_external_helpers};

use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use cranpose::{AppLauncher, WindowConfig, WindowModifierExt};
use cranpose_core::{delay, rememberMutableStateOf, LaunchedEffectAsync};
use cranpose_testing::{
    find_button_exact_in_semantics, find_text_by_prefix_in_semantics, print_semantics_with_bounds,
};
use cranpose_ui::{composable, Box, BoxSpec, Color, Modifier, Text, TextStyle};
use desktop_app::app;
use image::RgbaImage;
use text_showcase_external_helpers::{capture_x11_window, find_window_id};

const WINDOW_TITLE: &str = "Robot Presented Window Redraw";
const WINDOW_WIDTH: u32 = 800;
const WINDOW_HEIGHT: u32 = 600;
const MIN_CHANGED_PIXELS_AFTER_CLICK: usize = 500;
const TIMER_WINDOW_TITLE: &str = "Robot Hidden Window Timer";
const COVER_WINDOW_TITLE: &str = "Robot Owned Occlusion Cover";
const COVER_ENV: &str = "CRANPOSE_ROBOT_OCCLUSION_COVER";

#[derive(Default)]
struct Progress {
    enabled: AtomicBool,
    frames: AtomicU64,
    timers: AtomicU64,
    pause_timer: AtomicBool,
    timer_paused: AtomicBool,
}

struct CoverWindow(Child);

impl Drop for CoverWindow {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn xdotool(args: &[&str]) -> String {
    let result = Command::new("xdotool")
        .args(args)
        .output()
        .expect("xdotool");
    assert!(
        result.status.success(),
        "xdotool {args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8(result.stdout).expect("xdotool UTF-8 output")
}

fn cover_window() -> (CoverWindow, String) {
    let child = CoverWindow(
        Command::new(std::env::current_exe().expect("robot executable"))
            .arg("robot_presented_window_redraw")
            .env(COVER_ENV, "1")
            .stdout(Stdio::null())
            .spawn()
            .expect("launch owned cover"),
    );
    let pid = child.0.id().to_string();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let result = Command::new("xdotool")
            .args([
                "search",
                "--onlyvisible",
                "--pid",
                &pid,
                "--name",
                COVER_WINDOW_TITLE,
            ])
            .output()
            .expect("find owned cover");
        if let Some(id) = result
            .status
            .success()
            .then(|| String::from_utf8_lossy(&result.stdout))
            .and_then(|output| output.lines().next().map(str::to_owned))
        {
            xdotool(&["windowmove", &id, "0", "0"]);
            xdotool(&["windowraise", &id]);
            return (child, id);
        }
        assert!(Instant::now() < deadline, "owned cover did not appear");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn run_cover() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(30));
        std::process::exit(0);
    });
    AppLauncher::new()
        .with_title(COVER_WINDOW_TITLE)
        .with_size(860, 640)
        .with_headless(false)
        .run(|| {
            Box(
                Modifier::empty().fill_max_size().background(Color::WHITE),
                BoxSpec::default(),
                || {},
            );
        });
}

#[composable(no_skip)]
fn observed_app(progress: Arc<Progress>) {
    let frame_progress = Arc::clone(&progress);
    LaunchedEffectAsync((), move |scope| {
        Box::pin(async move {
            while !frame_progress.enabled.load(Ordering::Acquire) {
                delay(Duration::from_millis(20)).await;
            }
            let clock = scope.runtime().frame_clock();
            loop {
                clock.next_perpetual_frame().await;
                frame_progress.frames.fetch_add(1, Ordering::Relaxed);
            }
        })
    });
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::default(),
        move || {
            app::combined_app();
            timer_window(Arc::clone(&progress));
        },
    );
}

#[composable(no_skip)]
fn timer_window(progress: Arc<Progress>) {
    let ticks = rememberMutableStateOf(|| 0_u64);
    LaunchedEffectAsync((), move |_| {
        Box::pin(async move {
            loop {
                delay(Duration::from_millis(100)).await;
                if !progress.enabled.load(Ordering::Acquire) {
                    continue;
                }
                if progress.pause_timer.load(Ordering::Acquire) {
                    progress.timer_paused.store(true, Ordering::Release);
                    break;
                }
                let next = progress.timers.fetch_add(1, Ordering::Relaxed) + 1;
                ticks.set(next);
            }
        })
    });
    Box(
        Modifier::empty()
            .window(WindowConfig::new(TIMER_WINDOW_TITLE, 240.0, 120.0).with_position(920.0, 20.0))
            .fill_max_size()
            .background(Color::WHITE),
        BoxSpec::default(),
        move || {
            Text(
                format!("Timer: {}", ticks.get()),
                Modifier::empty().padding(16.0),
                TextStyle::default(),
            );
            for bit in 0..8 {
                let color = if ticks.get() & (1 << bit) == 0 {
                    Color::WHITE
                } else {
                    Color::BLACK
                };
                Box(
                    Modifier::empty()
                        .offset(16.0 + bit as f32 * 24.0, 80.0)
                        .size(cranpose_ui::Size {
                            width: 16.0,
                            height: 16.0,
                        })
                        .background(color),
                    BoxSpec::default(),
                    || {},
                );
            }
        },
    );
}

fn process_cpu_ticks() -> u64 {
    let stat = std::fs::read_to_string("/proc/self/stat").expect("process CPU accounting");
    let fields = stat
        .rsplit_once(')')
        .expect("process comm terminator")
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    fields[11].parse::<u64>().expect("utime") + fields[12].parse::<u64>().expect("stime")
}

fn check_hidden_window_progress(primary: &str, progress: &Progress, robot: &cranpose::Robot) {
    progress.enabled.store(true, Ordering::Release);
    let secondary = find_window_id(TIMER_WINDOW_TITLE);
    xdotool(&["windowmove", primary, "0", "0"]);
    let before = capture_window(&secondary, "timer-before-cover");
    let (cover, cover_id) = cover_window();
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        cranpose_services::current_lifecycle_state(),
        cranpose_services::LifecycleState::Stopped,
        "primary window must receive real OS occlusion"
    );
    let visible_frames = progress.frames.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(400));
    assert!(
        progress.frames.load(Ordering::Relaxed) > visible_frames,
        "visible secondary window must keep the shared frame clock running"
    );

    xdotool(&["windowsize", &cover_id, "1280", "800"]);
    xdotool(&["windowraise", &cover_id]);
    std::thread::sleep(Duration::from_millis(500));
    let hidden_frames = progress.frames.load(Ordering::Relaxed);
    let hidden_timers = progress.timers.load(Ordering::Relaxed);
    let cpu_before = process_cpu_ticks();
    let started = Instant::now();
    std::thread::sleep(Duration::from_millis(700));
    let frame_delta = progress.frames.load(Ordering::Relaxed) - hidden_frames;
    let timer_delta = progress.timers.load(Ordering::Relaxed) - hidden_timers;
    println!("all_hidden elapsed_ms={} frame_callbacks={} timer_callbacks={} cpu_ticks={} clock_ticks_per_second={}",
             started.elapsed().as_millis(), frame_delta, timer_delta, process_cpu_ticks() - cpu_before,
             String::from_utf8(Command::new("getconf").arg("CLK_TCK").output().expect("CLK_TCK").stdout).expect("CLK_TCK UTF-8").trim());
    progress.pause_timer.store(true, Ordering::Release);
    let pause_deadline = Instant::now() + Duration::from_secs(2);
    while !progress.timer_paused.load(Ordering::Acquire) {
        assert!(
            Instant::now() < pause_deadline,
            "hidden timer did not acknowledge pause"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let restored_value = progress.timers.load(Ordering::Relaxed);
    drop(cover);
    assert_eq!(
        frame_delta, 0,
        "fully occluded windows must not deliver ordinary animation frames"
    );
    assert!(
        timer_delta > 0,
        "wall-clock async timer must progress while all windows are hidden"
    );
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        progress.frames.load(Ordering::Relaxed) > hidden_frames,
        "uncover must resume the frame clock"
    );
    let after = capture_window(&secondary, "timer-after-uncover");
    for bit in 0..8 {
        let expected = if restored_value & (1 << bit) == 0 {
            255
        } else {
            0
        };
        let actual = after.image.get_pixel(24 + bit * 24, 88);
        assert_eq!(
            actual.0,
            [expected, expected, expected, 255],
            "restored pixels must show timer value {restored_value}, bit {bit}; capture={}",
            after.path.display()
        );
    }
    robot
        .get_semantics()
        .expect("query after restoring the visible window");
    std::thread::sleep(Duration::from_millis(200));
    let after_query = capture_window(&secondary, "timer-after-query");
    assert!(
        after.image == after_query.image,
        "uncover must present the hidden state before a robot query can pump a frame"
    );
    assert!(
        changed_pixel_count(&before.image, &after.image, 8) > 0,
        "uncover must present the timer state accumulated while hidden"
    );
    remove_capture(&before);
    remove_capture(&after);
    remove_capture(&after_query);
}

struct WindowCapture {
    image: RgbaImage,
    path: PathBuf,
}

fn capture_window(window_id: &str, tag: &str) -> WindowCapture {
    let path = capture_path(tag);
    let image = capture_x11_window(window_id, &path);
    WindowCapture { image, path }
}

fn capture_path(tag: &str) -> PathBuf {
    output_paths::diagnostic_path(&format!(
        "cranpose-presented-window-redraw-{}-{tag}.png",
        std::process::id()
    ))
}

fn changed_pixel_count(before: &RgbaImage, after: &RgbaImage, tolerance: u8) -> usize {
    assert_eq!(before.dimensions(), after.dimensions());
    before
        .pixels()
        .zip(after.pixels())
        .filter(|(lhs, rhs)| {
            lhs.0
                .iter()
                .zip(rhs.0.iter())
                .any(|(a, b)| a.abs_diff(*b) > tolerance)
        })
        .count()
}

fn remove_capture(capture: &WindowCapture) {
    let _ = std::fs::remove_file(&capture.path);
}

fn counter_value(robot: &cranpose::Robot) -> Option<i32> {
    find_text_by_prefix_in_semantics(robot, "Counter:")
        .and_then(|(_, _, _, _, text)| text.strip_prefix("Counter:")?.trim().parse().ok())
}

fn click_increment_until_state_changes(robot: &mut cranpose::Robot) {
    let initial = counter_value(robot).unwrap_or(0);
    for attempt in 1..=5 {
        let (x, y, w, h) =
            find_button_exact_in_semantics(robot, "Increment").expect("Increment button");
        let center_x = x + w * 0.5;
        let center_y = y + h * 0.5;
        println!(
            "increment_attempt={attempt} initial_counter={initial} button=({x:.1},{y:.1},{w:.1},{h:.1}) center=({center_x:.1},{center_y:.1})"
        );
        robot
            .move_to(center_x, center_y)
            .expect("move to Increment");
        robot
            .wait_for_present_frame()
            .expect("present hover frame before Increment click");
        robot.mouse_down().expect("press Increment");
        std::thread::sleep(Duration::from_millis(20));
        robot.mouse_up().expect("release Increment");

        for _ in 0..15 {
            robot.wait_for_idle().expect("idle after increment");
            if counter_value(robot).is_some_and(|value| value != initial) {
                return;
            }
            std::thread::sleep(Duration::from_millis(80));
        }

        eprintln!("Increment click attempt {attempt} did not update semantics yet");
    }

    if let Ok(semantics) = robot.get_semantics() {
        print_semantics_with_bounds(&semantics, 0);
    }
    panic!("Increment button did not update counter semantics");
}

fn wait_for_presented_change(window_id: &str, before: &WindowCapture) -> WindowCapture {
    let mut after = capture_window(window_id, "after");
    for attempt in 0..=12 {
        let changed = changed_pixel_count(&before.image, &after.image, 8);
        println!("changed_pixels_after_increment={changed} attempt={attempt}");
        if changed >= MIN_CHANGED_PIXELS_AFTER_CLICK {
            return after;
        }

        remove_capture(&after);
        std::thread::sleep(Duration::from_millis(80));
        after = capture_window(window_id, "after");
    }
    after
}

pub(crate) fn main() {
    env_logger::init();
    if std::env::var_os(COVER_ENV).is_some() {
        run_cover();
        return;
    }
    println!("=== Robot Presented Window Redraw ===");
    let progress = Arc::new(Progress::default());
    let observed = Arc::clone(&progress);

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(false)
        .with_frame_pacing_mode(cranpose_app_shell::FramePacingMode::Hard60)
        .with_test_driver(move |mut robot| {
            std::thread::sleep(Duration::from_millis(500));
            robot.wait_for_idle().expect("initial idle");

            let window_id = find_window_id(WINDOW_TITLE);
            let before = capture_window(&window_id, "before");

            click_increment_until_state_changes(&mut robot);

            let after = wait_for_presented_change(&window_id, &before);
            let changed = changed_pixel_count(&before.image, &after.image, 8);
            if changed < MIN_CHANGED_PIXELS_AFTER_CLICK {
                println!(
                    "FATAL: visible window did not present enough changed pixels after Increment; changed={changed} threshold={MIN_CHANGED_PIXELS_AFTER_CLICK} before={} after={}",
                    before.path.display(),
                    after.path.display()
                );
                std::process::exit(1);
            }

            remove_capture(&before);
            remove_capture(&after);
            check_hidden_window_progress(&window_id, &progress, &robot);
            robot.exit().expect("exit");
        })
        .run(move || observed_app(Arc::clone(&observed)));
}
