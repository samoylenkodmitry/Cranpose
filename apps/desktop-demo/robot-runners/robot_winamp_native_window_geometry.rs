use crate::output_paths;

use std::{
    cell::RefCell,
    collections::HashMap,
    process::Command,
    time::{Duration, Instant},
};

use cranpose::AppLauncher;
use cranpose_testing::find_button_in_semantics;
use desktop_app::app::{self, DemoTab, TEST_ACTIVE_TAB_STATE};
use image::RgbaImage;

const WINDOW_TITLE: &str = "Robot Winamp Native Geometry";
const MAIN_TITLE: &str = "Winamp";
const EQUALIZER_TITLE: &str = "Winamp Equalizer";
const PLAYLIST_TITLE: &str = "Winamp Playlist";
const WINAMP_TITLES: [&str; 3] = [MAIN_TITLE, EQUALIZER_TITLE, PLAYLIST_TITLE];
const MOVE_STEPS: usize = 12;
const MOVE_DX: i32 = 13;
const MOVE_DY: i32 = 7;
const FAST_MOVE_STEPS: usize = 8;
const FAST_MOVE_DX: i32 = 24;
const FAST_MOVE_DY: i32 = 11;
const OVERFLIGHT_STEPS: usize = 12;
const OVERFLIGHT_DX: i32 = 24;
const PIXEL_TRACE_STEPS: usize = 48;
const LONG_DRAG_TRACE_STEPS: usize = 128;
const LONG_DRAG_DX: i32 = 3;
const LONG_DRAG_DY: i32 = 1;
const LONG_DRAG_STEP_DELAY: Duration = Duration::from_millis(5);
const LONG_DRAG_MAX_POINTER_WINDOW_DRIFT: i32 = 12;
const LONG_DRAG_MAX_WINDOW_STEP: i32 = 18;
const TEAR_STEPS: usize = 8;
const TEAR_DX: i32 = 24;
const TEAR_DY: i32 = 11;
const SHORT_OF_TEAR_DX: i32 = 4;
const SHORT_OF_TEAR_DY: i32 = 3;
const DOCK_STEPS: usize = 8;
const STRETCH_STEPS: usize = 4;
const PLAYLIST_STRETCH: (f32, f32) = (25.0, 29.0);
const PLAYLIST_CORNER_INSET: i32 = 8;
const TORN_PANE_GAP: i32 = 80;
const TORN_PANE_ROW_GAP: i32 = 40;
const MAIN_GRIP: (i32, i32) = (120, 8);
const PANE_GRIP: (i32, i32) = (120, 6);
const PIXEL_TRACE_STALL_TIMEOUT: Duration = Duration::from_millis(90);
const FOLLOW_STEP_TIMEOUT: Duration = Duration::from_millis(500);
const TORN_WINDOW_TIMEOUT: Duration = Duration::from_millis(2_000);
const DOCK_TIMEOUT: Duration = Duration::from_millis(1_500);
const POST_RELEASE_STABILITY_TIMEOUT: Duration = Duration::from_millis(800);
const POST_RELEASE_STABILITY_POLL: Duration = Duration::from_millis(16);
const OFFSET_EPSILON: i32 = 3;
const FIND_WINDOW_POLL: Duration = Duration::from_millis(4);
const FIND_WINDOW_TIMEOUT: Duration = Duration::from_millis(12_000);
const SETUP_POSITION_TIMEOUT: Duration = Duration::from_millis(1200);
const CACHED_RESTORE_TIMEOUT: Duration = Duration::from_millis(1_000);
const TRANSPORT_CLICK_SETTLE: Duration = Duration::from_millis(160);
const WINAMP_MAIN_SKIN_WIDTH: f32 = 275.0;
const MAIN_SKIN_HEIGHT: f32 = 116.0;
const EQUALIZER_SKIN_HEIGHT: f32 = 116.0;
const PLAYLIST_SKIN_HEIGHT: f32 = 203.0;
const DOCKED_PANE_MAX_CHANGED_FRACTION: f32 = 0.02;
const TRANSPORT_Y: f32 = 88.0;
const TRANSPORT_BUTTON_WIDTH: f32 = 23.0;
const TRANSPORT_BUTTON_HEIGHT: f32 = 18.0;
const PLAY_X: f32 = 39.0;
const PAUSE_X: f32 = 62.0;
const STOP_X: f32 = 85.0;
const BUTTON_HOLD_SETTLE: Duration = Duration::from_millis(180);
const BUTTON_HOLD_SAMPLE_DELAY: Duration = Duration::from_millis(220);

thread_local! {
    static MONITOR_RECTS: RefCell<Option<Vec<WindowGeometry>>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WindowGeometry {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl WindowGeometry {
    fn origin(self) -> (i32, i32) {
        (self.x, self.y)
    }

    fn right(self) -> i32 {
        self.x + self.width
    }

    fn bottom(self) -> i32 {
        self.y + self.height
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PointerLocation {
    x: i32,
    y: i32,
    window: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pane {
    Equalizer,
    Playlist,
}

impl Pane {
    fn title(self) -> &'static str {
        match self {
            Self::Equalizer => EQUALIZER_TITLE,
            Self::Playlist => PLAYLIST_TITLE,
        }
    }

    fn skin_height(self) -> f32 {
        match self {
            Self::Equalizer => EQUALIZER_SKIN_HEIGHT,
            Self::Playlist => PLAYLIST_SKIN_HEIGHT,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Winamp {
    pid: u32,
    stack: u64,
    scale: f32,
}

#[derive(Clone, Copy, Debug)]
struct TornPane {
    pane: Pane,
    window: u64,
}

#[derive(Clone, Copy, Debug)]
struct DragTraceSample {
    pointer: PointerLocation,
    stack: WindowGeometry,
    elapsed: Duration,
}

impl Winamp {
    fn px(self, skin: f32) -> i32 {
        (skin * self.scale).round() as i32
    }

    fn stack_height(self, docked: &[Pane]) -> i32 {
        self.px(MAIN_SKIN_HEIGHT + docked.iter().map(|pane| pane.skin_height()).sum::<f32>())
    }

    fn pane_offset(self, pane: Pane, docked: &[Pane]) -> i32 {
        let above: f32 = docked
            .iter()
            .take_while(|held| **held != pane)
            .map(|held| held.skin_height())
            .sum();
        self.px(MAIN_SKIN_HEIGHT + above)
    }

    fn pane_window(self, pane: Pane) -> Option<u64> {
        find_window_ids(self.pid, pane.title()).into_iter().next()
    }
}

pub(crate) fn main() {
    env_logger::init();
    println!("=== Robot Winamp Native Window Geometry ===");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(1200, 800)
        .with_headless(false)
        .with_robot_app_hook(|name, argument| match (name.as_str(), argument.as_str()) {
            ("set-tab", "xkcd") => TEST_ACTIVE_TAB_STATE.with(|slot| {
                let active_tab = slot
                    .borrow()
                    .as_ref()
                    .copied()
                    .ok_or_else(|| "active tab state is not initialized".to_string())?;
                active_tab.set(DemoTab::Xkcd);
                Ok(None)
            }),
            _ => Err(format!("unsupported robot app hook {name}({argument})")),
        })
        .with_test_driver(|robot| {
            let pid = std::process::id();
            let host_window = find_app_window(pid);
            let host_origin = host_window_origin();
            move_window(host_window, host_origin.x, host_origin.y);
            std::thread::sleep(Duration::from_millis(250));
            robot.wait_for_idle().expect("host window positioned");

            let appeared_at = Instant::now();
            click_button_now(&robot, "Winamp");
            let stack = find_visible_window(pid, MAIN_TITLE);
            let appearance_elapsed = appeared_at.elapsed();
            robot.wait_for_idle().expect("initial Winamp idle");
            std::thread::sleep(Duration::from_millis(500));
            robot
                .wait_for_idle()
                .expect("initial native windows settled");
            let appeared = window_geometry(stack);
            println!(
                "the stack window appeared in {}ms: id={stack} {appeared:?}",
                appearance_elapsed.as_millis()
            );
            let winamp = Winamp {
                pid,
                stack,
                scale: appeared.width as f32 / WINAMP_MAIN_SKIN_WIDTH,
            };
            let all_docked = [Pane::Equalizer, Pane::Playlist];
            assert_stack_holds("appeared", winamp, &all_docked);
            assert_panes_not_torn("appeared", winamp, &all_docked);

            let origin = arrange_origin(winamp);
            place_window_for_setup("place-stack", stack, origin.x, origin.y);
            drag_stack_and_assert_it_follows(
                "drag-main",
                winamp,
                &all_docked,
                (MOVE_STEPS, MOVE_DX, MOVE_DY),
            );
            place_window_for_setup("place-stack", stack, origin.x + 10, origin.y + 5);
            drag_main_one_pixel_trace_and_assert_continuity("drag-main-pixel-trace", winamp);
            place_window_for_setup("place-stack", stack, origin.x + 20, origin.y + 10);
            drag_stack_and_assert_it_follows(
                "drag-main-fast",
                winamp,
                &all_docked,
                (FAST_MOVE_STEPS, FAST_MOVE_DX, FAST_MOVE_DY),
            );
            place_window_for_setup("place-stack", stack, origin.x + 30, origin.y + 15);
            drag_main_long_continuous_trace_and_assert_sync(
                "drag-main-long-trace",
                winamp,
                &all_docked,
            );

            place_window_for_setup("place-stack", stack, origin.x, origin.y);
            let equalizer = tear_pane("tear-equalizer", winamp, Pane::Equalizer, &all_docked);
            place_torn_panes(winamp, &[equalizer]);
            let equalizer_picture = capture_x11_window_image(
                equalizer.window,
                &diagnostic_png("tear-equalizer", "torn"),
            );
            let playlist = tear_pane("tear-playlist", winamp, Pane::Playlist, &[Pane::Playlist]);
            let torn = [equalizer, playlist];

            place_torn_panes(winamp, &torn);
            drag_main_over_torn_panes_and_assert_they_stay("drag-main-over-torn", winamp, &torn);
            place_window_for_setup("place-stack", stack, origin.x, origin.y);
            place_torn_panes(winamp, &torn);
            drag_torn_pane_alone("drag-equalizer", winamp, equalizer, &torn);
            place_torn_panes(winamp, &torn);
            drag_torn_pane_alone("drag-playlist", winamp, playlist, &torn);
            place_torn_panes(winamp, &torn);
            move_stack_with_window_manager_and_assert_panes_stay("wm-move-main", winamp, &torn);
            place_torn_panes(winamp, &torn);
            stretch_playlist_and_back("stretch-torn-playlist", winamp, playlist.window);

            place_window_for_setup("place-stack", stack, origin.x, origin.y);
            place_torn_panes(winamp, &torn);
            dock_pane("dock-equalizer", winamp, equalizer, &[], &[Pane::Equalizer]);
            assert_docked_pane_draws_in_its_slot(
                "dock-equalizer",
                winamp,
                Pane::Equalizer,
                &[Pane::Equalizer],
                &equalizer_picture,
            );

            dock_and_undock_and_assert_windows_restored(&robot, winamp, playlist);

            place_window_for_setup("place-stack", stack, origin.x, origin.y);
            place_torn_panes(winamp, &[playlist]);
            dock_pane(
                "dock-playlist",
                winamp,
                playlist,
                &[Pane::Equalizer],
                &all_docked,
            );
            assert_panes_not_torn("docked-again", winamp, &all_docked);
            stretch_playlist_and_back("stretch-docked-playlist", winamp, stack);

            hold_transport_button_and_assert_pressed_until_release("transport-button-hold", winamp);
            click_transport_buttons_and_assert_stack_remains("transport-buttons", winamp);
            drag_volume_to_zero_and_assert_stack_remains("volume-zero", winamp);

            robot
                .invoke_app_hook("set-tab", "xkcd")
                .expect("switch to XKCD tab");
            robot.wait_for_idle().expect("XKCD tab idle");
            assert_windows_absent(pid, "after tab switch");

            robot.exit().expect("exit");
        })
        .run(|| app::combined_app_with_initial_tab(Some(DemoTab::Counter)));
}

fn arrange_origin(winamp: Winamp) -> WindowGeometry {
    let stack = window_geometry(winamp.stack);
    let long_drag_travel_x = (LONG_DRAG_DX * LONG_DRAG_TRACE_STEPS as i32).max(0);
    let long_drag_travel_y = (LONG_DRAG_DY * LONG_DRAG_TRACE_STEPS as i32).max(0);
    let torn_panes_width = TORN_PANE_GAP + stack.width;
    let total_width = stack.width + long_drag_travel_x.max(torn_panes_width);
    let total_height = stack.height + long_drag_travel_y;
    let monitor = native_window_monitor();
    let desired_margin_x = 120.max(total_width / 2);
    let desired_margin_y = 120.max(total_height / 2);
    let margin_x = desired_margin_x.min(((monitor.width - total_width).max(0) / 2).max(8));
    let margin_y = desired_margin_y.min(((monitor.height - total_height).max(0) / 2).max(8));
    let max_x = monitor.x + monitor.width - total_width - margin_x;
    let max_y = monitor.y + monitor.height - total_height - margin_y;
    let fallback = WindowGeometry {
        x: (monitor.x + margin_x).min(max_x.max(monitor.x)),
        y: (monitor.y + margin_y).min(max_y.max(monitor.y)),
        width: stack.width,
        height: stack.height,
    };
    unobstructed_origin(monitor, total_width, total_height, fallback)
}

fn unobstructed_origin(
    monitor: WindowGeometry,
    total_width: i32,
    total_height: i32,
    fallback: WindowGeometry,
) -> WindowGeometry {
    let obstacles = visible_window_obstacles();
    if !intersects_any_obstacle(fallback, total_width, total_height, &obstacles) {
        return fallback;
    }

    let max_x = monitor.x + monitor.width - total_width;
    let max_y = monitor.y + monitor.height - total_height;
    let min_x = monitor.x + 8;
    let min_y = monitor.y + 8;
    if max_x < min_x || max_y < min_y {
        return fallback;
    }
    let step = 96.max(total_height / 3);
    let x_candidates = [
        fallback.x,
        min_x,
        (monitor.x + monitor.width / 2 - total_width / 2).clamp(min_x, max_x),
        max_x.saturating_sub(8),
    ];

    for x in x_candidates {
        let x = x.clamp(min_x, max_x);
        let mut y = min_y;
        while y <= max_y {
            let candidate = WindowGeometry {
                x,
                y,
                width: fallback.width,
                height: fallback.height,
            };
            if !intersects_any_obstacle(candidate, total_width, total_height, &obstacles) {
                println!("arrange origin avoided occupied area: fallback={fallback:?} selected={candidate:?}");
                return candidate;
            }
            y += step;
        }
    }

    fallback
}

fn visible_window_obstacles() -> Vec<WindowGeometry> {
    visible_windows_summary()
        .into_iter()
        .filter_map(|(_, title, geometry)| {
            let title = title.as_deref();
            if matches!(
                title,
                Some("Desktop" | "xfdesktop" | "Xfwm4" | WINDOW_TITLE)
            ) || title.is_some_and(|title| WINAMP_TITLES.contains(&title))
            {
                return None;
            }
            geometry.filter(|geometry| geometry.width > 1 && geometry.height > 1)
        })
        .collect()
}

fn intersects_any_obstacle(
    origin: WindowGeometry,
    total_width: i32,
    total_height: i32,
    obstacles: &[WindowGeometry],
) -> bool {
    let bounds = WindowGeometry {
        x: origin.x,
        y: origin.y,
        width: total_width,
        height: total_height,
    };
    obstacles
        .iter()
        .any(|obstacle| rectangles_intersect(bounds, *obstacle))
}

fn rectangles_intersect(first: WindowGeometry, second: WindowGeometry) -> bool {
    first.x < second.x + second.width
        && first.x + first.width > second.x
        && first.y < second.y + second.height
        && first.y + first.height > second.y
}

fn host_window_origin() -> WindowGeometry {
    let monitor = monitor_rects()
        .into_iter()
        .min_by_key(|monitor| monitor.x)
        .expect("at least one monitor");
    WindowGeometry {
        x: monitor.x + 160,
        y: monitor.y + 120,
        width: 1200,
        height: 800,
    }
}

fn native_window_monitor() -> WindowGeometry {
    let monitors = monitor_rects();
    if monitors.len() > 1 {
        return monitors
            .into_iter()
            .max_by_key(|monitor| monitor.x)
            .expect("at least one monitor");
    }
    monitors.into_iter().next().expect("at least one monitor")
}

fn find_app_window(pid: u32) -> u64 {
    let deadline = Instant::now() + FIND_WINDOW_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(id) = find_window_id_by_exact_title(pid, WINDOW_TITLE) {
            return id;
        }
        std::thread::sleep(FIND_WINDOW_POLL);
    }
    panic!(
        "host app window {WINDOW_TITLE:?} for pid {pid} not found; visible windows: {:?}",
        visible_windows_summary()
    );
}

fn find_visible_window(pid: u32, title: &str) -> u64 {
    let deadline = Instant::now() + FIND_WINDOW_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(id) = find_window_ids(pid, title).into_iter().next() {
            return id;
        }
        std::thread::sleep(FIND_WINDOW_POLL);
    }
    panic!(
        "native window {title:?} for pid {pid} not found; visible windows: {:?}",
        visible_windows_summary()
    );
}

fn find_window_id_by_exact_title(pid: u32, title: &str) -> Option<u64> {
    if let Some(mut windows) = wmctrl_window_ids_by_title(pid) {
        if let Some(id) = windows.remove(title) {
            return Some(id);
        }
    }

    find_window_ids(pid, title).into_iter().next()
}

fn wmctrl_window_ids_by_title(pid: u32) -> Option<HashMap<String, u64>> {
    let output = Command::new("wmctrl").arg("-lpG").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let pid = pid.to_string();
    let mut windows = HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() < 9 || parts[2] != pid {
            continue;
        }
        let title = parts[8..].join(" ");
        let id = u64::from_str_radix(parts[0].trim_start_matches("0x"), 16).ok()?;
        windows.insert(title, id);
    }
    (!windows.is_empty()).then_some(windows)
}

fn find_window_ids(pid: u32, title: &str) -> Vec<u64> {
    let pid = pid.to_string();
    let pid_matches = xdotool_search_title(
        ["search", "--onlyvisible", "--pid", &pid, "--name", title],
        title,
    );
    if !pid_matches.is_empty() {
        return pid_matches;
    }

    visible_windows_summary()
        .into_iter()
        .filter_map(|(id, candidate_title, _)| {
            (candidate_title.as_deref() == Some(title)).then_some(id)
        })
        .collect()
}

fn xdotool_search_title<const N: usize>(args: [&str; N], title: &str) -> Vec<u64> {
    let output = Command::new("xdotool")
        .args(args)
        .output()
        .expect("xdotool search");
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u64>().ok())
        .filter(|id| window_title(*id).as_deref() == Some(title))
        .collect()
}

fn monitor_rects() -> Vec<WindowGeometry> {
    MONITOR_RECTS.with(|slot| {
        if let Some(monitors) = slot.borrow().as_ref().cloned() {
            return monitors;
        }
        let monitors = load_monitor_rects();
        *slot.borrow_mut() = Some(monitors.clone());
        monitors
    })
}

fn load_monitor_rects() -> Vec<WindowGeometry> {
    if let Some(monitors) = xrandr_monitor_rects() {
        return monitors;
    }

    let output = Command::new("xdotool")
        .arg("getdisplaygeometry")
        .output()
        .expect("xdotool getdisplaygeometry");
    assert!(output.status.success(), "xdotool getdisplaygeometry failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut parts = stdout.split_whitespace();
    vec![WindowGeometry {
        x: 0,
        y: 0,
        width: parts
            .next()
            .expect("display width")
            .parse()
            .expect("display width"),
        height: parts
            .next()
            .expect("display height")
            .parse()
            .expect("display height"),
    }]
}

fn xrandr_monitor_rects() -> Option<Vec<WindowGeometry>> {
    let output = Command::new("xrandr").arg("--listmonitors").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let monitors: Vec<_> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_monitor_rect)
        .collect();
    (!monitors.is_empty()).then_some(monitors)
}

fn parse_monitor_rect(line: &str) -> Option<WindowGeometry> {
    let geometry = line
        .split_whitespace()
        .find(|part| part.contains('x') && part.contains('+'))?;
    let (geometry, y) = geometry.rsplit_once('+')?;
    let (size, x) = geometry.rsplit_once('+')?;
    let (width, rest) = size.split_once('/')?;
    let (_, height) = rest.split_once('x')?;
    let height = height.split_once('/').map_or(height, |(height, _)| height);
    Some(WindowGeometry {
        x: x.parse().ok()?,
        y: y.parse().ok()?,
        width: width.parse().ok()?,
        height: height.parse().ok()?,
    })
}

fn assert_windows_absent(pid: u32, label: &str) {
    for _ in 0..20 {
        if WINAMP_TITLES
            .iter()
            .all(|title| find_window_ids(pid, title).is_empty())
        {
            println!("{label}: native windows absent");
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let visible: Vec<_> = WINAMP_TITLES
        .iter()
        .flat_map(|title| {
            find_window_ids(pid, title)
                .into_iter()
                .map(move |id| (*title, id, window_geometry(id)))
        })
        .collect();
    panic!("{label}: native Winamp windows are still visible for pid {pid}: {visible:?}");
}

fn assert_stack_holds(label: &str, winamp: Winamp, docked: &[Pane]) -> WindowGeometry {
    let expected_width = winamp.px(WINAMP_MAIN_SKIN_WIDTH);
    let expected_height = winamp.stack_height(docked);
    let deadline = Instant::now() + DOCK_TIMEOUT;
    let mut stack = window_geometry(winamp.stack);
    while Instant::now() < deadline {
        stack = window_geometry(winamp.stack);
        if (stack.width - expected_width).abs() <= 1 && (stack.height - expected_height).abs() <= 1
        {
            println!("{label}: the stack holds {docked:?} at {stack:?}");
            return stack;
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    panic!(
        "{label}: the stack is not the size of the main window with {docked:?} docked under it \
         expected={expected_width}x{expected_height} actual={stack:?}"
    );
}

fn assert_panes_not_torn(label: &str, winamp: Winamp, docked: &[Pane]) {
    for pane in docked {
        assert_eq!(
            winamp.pane_window(*pane),
            None,
            "{label}: the docked {pane:?} has a window of its own"
        );
    }
}

fn place_torn_panes(winamp: Winamp, torn: &[TornPane]) {
    let stack = window_geometry(winamp.stack);
    let x = stack.right() + TORN_PANE_GAP;
    let mut y = stack.y;
    for torn_pane in torn {
        let placed = place_window_for_setup(
            &format!("place-torn-{:?}", torn_pane.pane),
            torn_pane.window,
            x,
            y,
        );
        y = placed.bottom() + TORN_PANE_ROW_GAP;
    }
    println!(
        "place torn panes: stack={stack:?} torn={:?}",
        torn.iter()
            .map(|torn_pane| (torn_pane.pane, window_geometry(torn_pane.window)))
            .collect::<Vec<_>>()
    );
}

fn place_window_for_setup(label: &str, window_id: u64, x: i32, y: i32) -> WindowGeometry {
    activate_window(window_id);
    move_window(window_id, x, y);
    let mut last_move_requested = Instant::now();
    let deadline = Instant::now() + SETUP_POSITION_TIMEOUT;
    let mut last_geometry = window_geometry(window_id);
    while Instant::now() < deadline {
        let geometry = window_geometry(window_id);
        last_geometry = geometry;
        if (geometry.x - x).abs() <= OFFSET_EPSILON && (geometry.y - y).abs() <= OFFSET_EPSILON {
            std::thread::sleep(Duration::from_millis(180));
            let stable = window_geometry(window_id);
            if (stable.x - x).abs() <= OFFSET_EPSILON && (stable.y - y).abs() <= OFFSET_EPSILON {
                return stable;
            }
            last_geometry = stable;
        }
        if last_move_requested.elapsed() >= Duration::from_millis(50) {
            move_window(window_id, x, y);
            last_move_requested = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(8));
    }

    println!(
        "{label}: OS window did not reach setup position target=({x},{y}) actual={last_geometry:?}; continuing from actual setup position"
    );
    last_geometry
}

fn press_in_window(label: &str, window_id: u64, grip: (i32, i32)) -> PointerLocation {
    activate_window(window_id);
    mousemove_in_window_exact(label, window_id, grip.0, grip.1);
    std::thread::sleep(Duration::from_millis(100));
    xdotool(["mousedown", "1"]);
    std::thread::sleep(Duration::from_millis(60));
    pointer_location()
}

fn release_pointer(settle: Duration) {
    xdotool(["mouseup", "1"]);
    std::thread::sleep(settle);
}

fn drag_stack_and_assert_it_follows(
    label: &str,
    winamp: Winamp,
    docked: &[Pane],
    (steps, dx, dy): (usize, i32, i32),
) {
    let initial = assert_stack_holds(label, winamp, docked);
    let pressed = press_in_window(label, winamp.stack, MAIN_GRIP);

    for step in 1..=steps {
        let moved_at = Instant::now();
        drag_pointer_by(dx, dy);
        let current = wait_for_window_under_pointer(
            label,
            step,
            winamp.stack,
            (pressed.x - initial.x, pressed.y - initial.y),
        );
        assert_eq!(
            (current.width, current.height),
            (initial.width, initial.height),
            "{label} step {step}: the stack changed size while it was dragged, so a docked pane \
             left it"
        );
        println!(
            "{label} step {step}: followed in {}ms {current:?}",
            moved_at.elapsed().as_millis()
        );
    }

    release_pointer(Duration::from_millis(120));
    let final_stack = window_geometry(winamp.stack);
    assert_window_moved(label, initial, final_stack);
    assert_windows_stop_after_release(label, &[winamp.stack]);
}

fn wait_for_window_under_pointer(
    label: &str,
    step: usize,
    window_id: u64,
    grab: (i32, i32),
) -> WindowGeometry {
    let deadline = Instant::now() + FOLLOW_STEP_TIMEOUT;
    let pointer = pointer_location();
    let expected = (pointer.x - grab.0, pointer.y - grab.1);
    let mut current = window_geometry(window_id);
    while Instant::now() < deadline {
        current = window_geometry(window_id);
        if origin_close(current.origin(), expected) {
            return current;
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    panic!(
        "{label} step {step}: the window did not stay under the pointer expected_origin={expected:?} \
         actual={current:?} pointer={pointer:?}"
    );
}

fn origin_close(actual: (i32, i32), expected: (i32, i32)) -> bool {
    (actual.0 - expected.0).abs() <= OFFSET_EPSILON
        && (actual.1 - expected.1).abs() <= OFFSET_EPSILON
}

fn drag_main_one_pixel_trace_and_assert_continuity(label: &str, winamp: Winamp) {
    press_in_window(label, winamp.stack, MAIN_GRIP);

    let mut trace = Vec::with_capacity(PIXEL_TRACE_STEPS + 1);
    trace.push(DragTraceSample {
        pointer: pointer_location(),
        stack: window_geometry(winamp.stack),
        elapsed: Duration::ZERO,
    });

    for step in 1..=PIXEL_TRACE_STEPS {
        let previous = *trace.last().expect("previous trace sample");
        mousemove_absolute(previous.pointer.x + 1, previous.pointer.y);
        trace.push(wait_for_one_pixel_drag_step(label, step, winamp, previous));
        std::thread::sleep(Duration::from_millis(8));
    }

    release_pointer(Duration::from_millis(120));
    print_drag_trace(label, &trace);
    assert_pixel_drag_trace_continuity(label, &trace);
    assert_windows_stop_after_release(label, &[winamp.stack]);
}

fn wait_for_one_pixel_drag_step(
    label: &str,
    step: usize,
    winamp: Winamp,
    previous: DragTraceSample,
) -> DragTraceSample {
    let started = Instant::now();
    loop {
        let sample = DragTraceSample {
            pointer: pointer_location(),
            stack: window_geometry(winamp.stack),
            elapsed: started.elapsed(),
        };
        let pointer_dx = sample.pointer.x - previous.pointer.x;
        let pointer_dy = sample.pointer.y - previous.pointer.y;
        let stack_dx = sample.stack.x - previous.stack.x;
        let stack_dy = sample.stack.y - previous.stack.y;

        assert!(
            !(pointer_dx > 1 || pointer_dy != 0),
            "{label} step {step}: robot pointer moved incorrectly previous={previous:?} sample={sample:?}"
        );
        assert!(
            !(stack_dx > 1 || stack_dy != 0),
            "{label} step {step}: staircase jump detected previous={previous:?} sample={sample:?}"
        );
        assert_eq!(
            (sample.stack.width, sample.stack.height),
            (previous.stack.width, previous.stack.height),
            "{label} step {step}: the stack changed size while it was dragged"
        );
        if pointer_dx == 1 && stack_dx == 1 && stack_dy == 0 {
            return sample;
        }
        assert!(
            sample.elapsed <= PIXEL_TRACE_STALL_TIMEOUT,
            "{label} step {step}: one-pixel drag did not complete within {}ms previous={previous:?} last={sample:?}",
            PIXEL_TRACE_STALL_TIMEOUT.as_millis()
        );

        std::thread::sleep(Duration::from_millis(1));
    }
}

fn print_drag_trace(label: &str, trace: &[DragTraceSample]) {
    for (index, sample) in trace.iter().enumerate() {
        println!(
            "{label} trace {index:02}: dt={}ms pointer=({}, {}) stack=({}, {}) {}x{}",
            sample.elapsed.as_millis(),
            sample.pointer.x,
            sample.pointer.y,
            sample.stack.x,
            sample.stack.y,
            sample.stack.width,
            sample.stack.height,
        );
    }
}

fn assert_pixel_drag_trace_continuity(label: &str, trace: &[DragTraceSample]) {
    assert!(
        trace.len() == PIXEL_TRACE_STEPS + 1,
        "{label}: expected {} trace samples, got {}",
        PIXEL_TRACE_STEPS + 1,
        trace.len()
    );

    for index in 1..trace.len() {
        let previous = trace[index - 1];
        let current = trace[index];
        assert_eq!(
            (
                current.pointer.x - previous.pointer.x,
                current.pointer.y - previous.pointer.y
            ),
            (1, 0),
            "{label} sample {index}: robot pointer did not advance by exactly one pixel trace={trace:?}"
        );
        assert_eq!(
            (
                current.stack.x - previous.stack.x,
                current.stack.y - previous.stack.y
            ),
            (1, 0),
            "{label} sample {index}: the stack did not follow the one-pixel pointer step exactly"
        );
    }

    let first = trace.first().expect("first trace sample").stack;
    let last = trace.last().expect("last trace sample").stack;
    let total_dx = last.x - first.x;
    assert_eq!(
        total_dx, PIXEL_TRACE_STEPS as i32,
        "{label}: total stack movement should match pointer pixels expected={PIXEL_TRACE_STEPS} actual={total_dx} trace={trace:?}"
    );
}

fn drag_main_long_continuous_trace_and_assert_sync(label: &str, winamp: Winamp, docked: &[Pane]) {
    let initial = assert_stack_holds(label, winamp, docked);
    press_in_window(label, winamp.stack, MAIN_GRIP);

    let origin_pointer = pointer_location();
    let started = Instant::now();
    let mut trace = Vec::with_capacity(LONG_DRAG_TRACE_STEPS + 2);
    trace.push(DragTraceSample {
        pointer: origin_pointer,
        stack: window_geometry(winamp.stack),
        elapsed: Duration::ZERO,
    });

    for step in 1..=LONG_DRAG_TRACE_STEPS {
        mousemove_absolute_unsynced(
            origin_pointer.x + LONG_DRAG_DX * step as i32,
            origin_pointer.y + LONG_DRAG_DY * step as i32,
        );
        std::thread::sleep(LONG_DRAG_STEP_DELAY);
        trace.push(DragTraceSample {
            pointer: pointer_location(),
            stack: window_geometry(winamp.stack),
            elapsed: started.elapsed(),
        });
    }

    std::thread::sleep(Duration::from_millis(80));
    trace.push(DragTraceSample {
        pointer: pointer_location(),
        stack: window_geometry(winamp.stack),
        elapsed: started.elapsed(),
    });
    release_pointer(Duration::from_millis(160));

    assert_long_drag_trace_sync(label, initial, &trace);
    assert_windows_stop_after_release(label, &[winamp.stack]);
}

fn assert_long_drag_trace_sync(label: &str, initial: WindowGeometry, trace: &[DragTraceSample]) {
    assert!(
        trace.len() >= LONG_DRAG_TRACE_STEPS,
        "{label}: expected a long drag trace, got {} samples",
        trace.len()
    );
    let first = trace.first().expect("first long trace sample");
    let mut max_drift_x = 0;
    let mut max_drift_y = 0;

    for index in 1..trace.len() {
        let previous = &trace[index - 1];
        let current = &trace[index];
        let pointer_dx = current.pointer.x - first.pointer.x;
        let pointer_dy = current.pointer.y - first.pointer.y;
        let stack_dx = current.stack.x - first.stack.x;
        let stack_dy = current.stack.y - first.stack.y;
        let step_dx = current.stack.x - previous.stack.x;
        let step_dy = current.stack.y - previous.stack.y;
        let drift_x = (pointer_dx - stack_dx).abs();
        let drift_y = (pointer_dy - stack_dy).abs();
        max_drift_x = max_drift_x.max(drift_x);
        max_drift_y = max_drift_y.max(drift_y);

        assert_eq!(
            (current.stack.width, current.stack.height),
            (initial.width, initial.height),
            "{label} sample {index}: the stack changed size while it was dragged current={current:?}"
        );
        assert!(
            current.stack.x + 1 >= previous.stack.x,
            "{label} sample {index}: the stack reversed on a monotonic drag previous={previous:?} current={current:?}"
        );
        assert!(
            current.stack.y + 1 >= previous.stack.y,
            "{label} sample {index}: the stack reversed vertically on a monotonic drag previous={previous:?} current={current:?}"
        );
        assert!(
            step_dx <= LONG_DRAG_MAX_WINDOW_STEP && step_dy <= LONG_DRAG_MAX_WINDOW_STEP,
            "{label} sample {index}: the stack jumped too far in one sample step=({step_dx},{step_dy}) previous={previous:?} current={current:?}"
        );
        if index > 8 {
            assert!(
                drift_x <= LONG_DRAG_MAX_POINTER_WINDOW_DRIFT
                    && drift_y <= LONG_DRAG_MAX_POINTER_WINDOW_DRIFT,
                "{label} sample {index}: pointer/window drift exceeded {LONG_DRAG_MAX_POINTER_WINDOW_DRIFT}px drift=({drift_x},{drift_y}) max_so_far=({max_drift_x},{max_drift_y}) first={first:?} current={current:?}"
            );
        }
    }

    let last = trace.last().expect("last long trace sample");
    assert_window_moved(label, first.stack, last.stack);
    println!("{label}: max pointer/window drift=({max_drift_x},{max_drift_y})");
}

fn tear_pane(label: &str, winamp: Winamp, pane: Pane, docked: &[Pane]) -> TornPane {
    let before = assert_stack_holds(label, winamp, docked);
    let offset = winamp.pane_offset(pane, docked);
    let pressed = press_in_window(label, winamp.stack, (PANE_GRIP.0, offset + PANE_GRIP.1));
    let grab = (pressed.x - before.x, pressed.y - (before.y + offset));

    drag_pointer_by(SHORT_OF_TEAR_DX, SHORT_OF_TEAR_DY);
    std::thread::sleep(Duration::from_millis(160));
    assert_eq!(
        winamp.pane_window(pane),
        None,
        "{label}: a press that wandered ({SHORT_OF_TEAR_DX},{SHORT_OF_TEAR_DY}) tore the {pane:?} off"
    );
    assert_eq!(
        window_geometry(winamp.stack),
        before,
        "{label}: a press on a docked pane's title moved or resized the stack"
    );

    let mut window = None;
    for step in 1..=TEAR_STEPS {
        drag_pointer_by(TEAR_DX, TEAR_DY);
        let torn = match window {
            Some(torn) => torn,
            None => wait_for_torn_window(label, winamp, pane),
        };
        window = Some(torn);
        let current = wait_for_window_under_pointer(label, step, torn, grab);
        println!("{label} step {step}: the torn {pane:?} is under the pointer at {current:?}");
    }
    release_pointer(Duration::from_millis(160));

    let window = window.expect("the pane tore off");
    let remaining: Vec<Pane> = docked
        .iter()
        .copied()
        .filter(|held| *held != pane)
        .collect();
    let after = assert_stack_holds(label, winamp, &remaining);
    assert_eq!(
        after.origin(),
        before.origin(),
        "{label}: tearing a pane off moved the stack"
    );
    assert_windows_stop_after_release(label, &[winamp.stack, window]);
    TornPane { pane, window }
}

fn wait_for_torn_window(label: &str, winamp: Winamp, pane: Pane) -> u64 {
    let deadline = Instant::now() + TORN_WINDOW_TIMEOUT;
    while Instant::now() < deadline {
        if let Some(window) = winamp.pane_window(pane) {
            return window;
        }
        std::thread::sleep(FIND_WINDOW_POLL);
    }
    panic!(
        "{label}: the {pane:?} carried past the tear reach did not come out into a window of its own; \
         visible windows: {:?}",
        visible_windows_summary()
    );
}

fn drag_main_over_torn_panes_and_assert_they_stay(label: &str, winamp: Winamp, torn: &[TornPane]) {
    let initial = window_geometry(winamp.stack);
    let panes: Vec<_> = torn
        .iter()
        .map(|torn_pane| window_geometry(torn_pane.window))
        .collect();
    let pressed = press_in_window(label, winamp.stack, MAIN_GRIP);
    let grab = (pressed.x - initial.x, pressed.y - initial.y);

    for step in 1..=OVERFLIGHT_STEPS {
        drag_pointer_by(OVERFLIGHT_DX, 0);
        let current = wait_for_window_under_pointer(label, step, winamp.stack, grab);
        println!("{label} step {step}: {current:?}");
        assert_torn_panes_unmoved(label, step, torn, &panes);
    }

    release_pointer(Duration::from_millis(160));
    assert_torn_panes_unmoved(label, OVERFLIGHT_STEPS + 1, torn, &panes);
    assert_window_moved(label, initial, window_geometry(winamp.stack));
    let mut windows = vec![winamp.stack];
    windows.extend(torn.iter().map(|torn_pane| torn_pane.window));
    assert_windows_stop_after_release(label, &windows);
}

fn assert_torn_panes_unmoved(
    label: &str,
    step: usize,
    torn: &[TornPane],
    expected: &[WindowGeometry],
) {
    for (torn_pane, expected) in torn.iter().zip(expected) {
        assert_eq!(
            window_geometry(torn_pane.window),
            *expected,
            "{label} step {step}: the torn {:?} moved with the main window",
            torn_pane.pane
        );
    }
}

fn drag_torn_pane_alone(label: &str, winamp: Winamp, dragged: TornPane, torn: &[TornPane]) {
    let initial = window_geometry(dragged.window);
    let stack = window_geometry(winamp.stack);
    let others: Vec<TornPane> = torn
        .iter()
        .copied()
        .filter(|torn_pane| torn_pane.window != dragged.window)
        .collect();
    let other_geometries: Vec<_> = others
        .iter()
        .map(|torn_pane| window_geometry(torn_pane.window))
        .collect();
    let pressed = press_in_window(label, dragged.window, PANE_GRIP);
    let grab = (pressed.x - initial.x, pressed.y - initial.y);

    for step in 1..=MOVE_STEPS {
        drag_pointer_by(MOVE_DX, MOVE_DY);
        let current = wait_for_window_under_pointer(label, step, dragged.window, grab);
        println!("{label} step {step}: {current:?}");
        assert_eq!(
            window_geometry(winamp.stack),
            stack,
            "{label} step {step}: the stack moved with a torn pane"
        );
        assert_torn_panes_unmoved(label, step, &others, &other_geometries);
    }

    release_pointer(Duration::from_millis(160));
    let released = window_geometry(dragged.window);
    assert_window_moved(label, initial, released);
    assert_eq!(
        winamp.pane_window(dragged.pane),
        Some(dragged.window),
        "{label}: a torn pane let go away from the stack went back into it"
    );
    let mut windows = vec![winamp.stack];
    windows.extend(torn.iter().map(|torn_pane| torn_pane.window));
    assert_windows_stop_after_release(label, &windows);
}

fn move_stack_with_window_manager_and_assert_panes_stay(
    label: &str,
    winamp: Winamp,
    torn: &[TornPane],
) {
    if !window_manager_supports_net_active_window() {
        println!("{label}: skipping external window-manager move; _NET_ACTIVE_WINDOW unsupported");
        return;
    }

    let initial = window_geometry(winamp.stack);
    let panes: Vec<_> = torn
        .iter()
        .map(|torn_pane| window_geometry(torn_pane.window))
        .collect();
    let target = (initial.x + 31, initial.y + 19);
    move_window(winamp.stack, target.0, target.1);

    let deadline = Instant::now() + SETUP_POSITION_TIMEOUT;
    let mut current = window_geometry(winamp.stack);
    while Instant::now() < deadline && !origin_close(current.origin(), target) {
        std::thread::sleep(Duration::from_millis(8));
        current = window_geometry(winamp.stack);
    }
    println!("{label}: {current:?}");
    assert!(
        origin_close(current.origin(), target),
        "{label}: the window manager did not move the stack target={target:?} actual={current:?}"
    );
    assert_torn_panes_unmoved(label, 1, torn, &panes);
    let mut windows = vec![winamp.stack];
    windows.extend(torn.iter().map(|torn_pane| torn_pane.window));
    assert_windows_stop_after_release(label, &windows);
}

fn window_manager_supports_net_active_window() -> bool {
    let Some(output) = Command::new("xprop")
        .args(["-root", "_NET_SUPPORTED"])
        .output()
        .ok()
    else {
        return true;
    };
    if !output.status.success() {
        return true;
    }
    String::from_utf8_lossy(&output.stdout).contains("_NET_ACTIVE_WINDOW")
}

fn dock_pane(
    label: &str,
    winamp: Winamp,
    torn: TornPane,
    docked_before: &[Pane],
    docked_after: &[Pane],
) {
    let stack = assert_stack_holds(label, winamp, docked_before);
    let initial = window_geometry(torn.window);
    let pressed = press_in_window(label, torn.window, PANE_GRIP);
    let grab = (pressed.x - initial.x, pressed.y - initial.y);
    let target = (stack.x + grab.0, stack.bottom() + grab.1);

    for step in 1..=DOCK_STEPS {
        let fraction = step as f32 / DOCK_STEPS as f32;
        mousemove_absolute(
            pressed.x + ((target.0 - pressed.x) as f32 * fraction).round() as i32,
            pressed.y + ((target.1 - pressed.y) as f32 * fraction).round() as i32,
        );
        let current = wait_for_window_under_pointer(label, step, torn.window, grab);
        println!("{label} step {step}: {current:?}");
    }
    std::thread::sleep(Duration::from_millis(120));
    let let_go_at = window_geometry(torn.window);
    println!(
        "{label}: letting the {:?} go at {let_go_at:?} on the stack's bottom edge {stack:?}",
        torn.pane
    );
    release_pointer(Duration::from_millis(60));

    let deadline = Instant::now() + DOCK_TIMEOUT;
    while Instant::now() < deadline && winamp.pane_window(torn.pane).is_some() {
        std::thread::sleep(Duration::from_millis(8));
    }
    assert_eq!(
        winamp.pane_window(torn.pane),
        None,
        "{label}: the {:?} let go on the stack's bottom edge kept its own window",
        torn.pane
    );
    let after = assert_stack_holds(label, winamp, docked_after);
    assert_eq!(
        after.origin(),
        stack.origin(),
        "{label}: docking a pane moved the stack"
    );
    assert_windows_stop_after_release(label, &[winamp.stack]);
}

fn assert_docked_pane_draws_in_its_slot(
    label: &str,
    winamp: Winamp,
    pane: Pane,
    docked: &[Pane],
    torn_picture: &RgbaImage,
) {
    activate_window(winamp.stack);
    std::thread::sleep(Duration::from_millis(120));
    let stack = capture_x11_window_image(winamp.stack, &diagnostic_png(label, "stack"));
    let offset = u32::try_from(winamp.pane_offset(pane, docked)).expect("pane offset");
    assert!(
        stack.width() >= torn_picture.width() && stack.height() >= offset + torn_picture.height(),
        "{label}: the stack {}x{} has no room for the {pane:?} at {offset}",
        stack.width(),
        stack.height()
    );
    let slot = image::imageops::crop_imm(
        &stack,
        0,
        offset,
        torn_picture.width(),
        torn_picture.height(),
    )
    .to_image();
    let changed = image_changed_pixels(torn_picture, &slot, 8);
    let allowed =
        (torn_picture.width() * torn_picture.height()) as f32 * DOCKED_PANE_MAX_CHANGED_FRACTION;
    println!("{label}: the docked {pane:?} differs from its torn window in {changed} pixels");
    assert!(
        changed as f32 <= allowed,
        "{label}: the stack does not draw the docked {pane:?} in its slot at {offset}: \
         {changed} pixels differ from the pane's own window"
    );
}

fn stretch_playlist_and_back(label: &str, winamp: Winamp, window_id: u64) {
    let initial = window_geometry(window_id);
    let stretch = (winamp.px(PLAYLIST_STRETCH.0), winamp.px(PLAYLIST_STRETCH.1));
    drag_playlist_corner(label, window_id, stretch);
    let stretched = wait_for_window_size(
        label,
        window_id,
        (initial.width + stretch.0, initial.height + stretch.1),
    );
    assert_eq!(
        stretched.origin(),
        initial.origin(),
        "{label}: stretching the playlist moved the window it is in"
    );
    drag_playlist_corner(label, window_id, (-stretch.0, -stretch.1));
    let restored = wait_for_window_size(label, window_id, (initial.width, initial.height));
    assert_eq!(
        restored.origin(),
        initial.origin(),
        "{label}: shrinking the playlist moved the window it is in"
    );
    assert_windows_stop_after_release(label, &[window_id]);
}

fn drag_playlist_corner(label: &str, window_id: u64, (dx, dy): (i32, i32)) {
    let geometry = window_geometry(window_id);
    let pressed = press_in_window(
        label,
        window_id,
        (
            geometry.width - PLAYLIST_CORNER_INSET,
            geometry.height - PLAYLIST_CORNER_INSET,
        ),
    );
    for step in 1..=STRETCH_STEPS {
        let fraction = step as f32 / STRETCH_STEPS as f32;
        mousemove_absolute(
            pressed.x + (dx as f32 * fraction).round() as i32,
            pressed.y + (dy as f32 * fraction).round() as i32,
        );
        std::thread::sleep(Duration::from_millis(40));
    }
    std::thread::sleep(Duration::from_millis(80));
    release_pointer(Duration::from_millis(120));
}

fn wait_for_window_size(label: &str, window_id: u64, size: (i32, i32)) -> WindowGeometry {
    let deadline = Instant::now() + DOCK_TIMEOUT;
    let mut current = window_geometry(window_id);
    while Instant::now() < deadline {
        current = window_geometry(window_id);
        if (current.width - size.0).abs() <= 1 && (current.height - size.1).abs() <= 1 {
            println!("{label}: {current:?}");
            return current;
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    panic!("{label}: the playlist's corner did not size its window to {size:?}: {current:?}");
}

fn dock_and_undock_and_assert_windows_restored(
    robot: &cranpose::Robot,
    winamp: Winamp,
    playlist: TornPane,
) {
    let label = "dock-undock";
    let docked = [Pane::Equalizer];
    let stack = assert_stack_holds(label, winamp, &docked);
    let torn = window_geometry(playlist.window);

    click_button(robot, "Dock");
    assert_windows_absent(winamp.pid, "after Dock");

    let restore_started = Instant::now();
    click_button_now(robot, "Undock");
    let restored_stack = find_visible_window(winamp.pid, MAIN_TITLE);
    let restored_playlist = find_visible_window(winamp.pid, PLAYLIST_TITLE);
    let restore_elapsed = restore_started.elapsed();
    println!(
        "{label}: native windows restored after Undock in {}ms: stack={restored_stack} playlist={restored_playlist}",
        restore_elapsed.as_millis()
    );
    assert_eq!(
        (restored_stack, restored_playlist),
        (winamp.stack, playlist.window),
        "{label}: Undock made new native windows instead of showing the ones Dock put away"
    );
    assert!(
        restore_elapsed <= CACHED_RESTORE_TIMEOUT,
        "{label}: Undock restore took {}ms, expected the windows Dock put away back within {}ms",
        restore_elapsed.as_millis(),
        CACHED_RESTORE_TIMEOUT.as_millis()
    );
    robot.wait_for_idle().expect("Undock idle");
    let restored = assert_stack_holds(label, winamp, &docked);
    assert!(
        origin_close(restored.origin(), stack.origin()),
        "{label}: the stack came back somewhere else before={stack:?} after={restored:?}"
    );
    let restored_torn = window_geometry(playlist.window);
    assert!(
        origin_close(restored_torn.origin(), torn.origin()),
        "{label}: the torn playlist came back somewhere else before={torn:?} after={restored_torn:?}"
    );
    assert_panes_not_torn(label, winamp, &docked);
}

fn drag_volume_to_zero_and_assert_stack_remains(label: &str, winamp: Winamp) {
    let initial = window_geometry(winamp.stack);
    let start_x = initial.x + winamp.px(107.0 + 54.0);
    let end_x = initial.x + winamp.px(107.0);
    let y = initial.y + winamp.px(57.0 + 5.0);

    activate_window(winamp.stack);
    mousemove_absolute(start_x, y);
    std::thread::sleep(Duration::from_millis(80));
    xdotool(["mousedown", "1"]);
    std::thread::sleep(Duration::from_millis(80));
    mousemove_absolute(end_x, y);
    std::thread::sleep(Duration::from_millis(120));
    release_pointer(Duration::from_millis(180));

    assert_eq!(
        window_geometry(winamp.stack),
        initial,
        "{label}: volume drag moved, resized or hid the stack"
    );
}

fn click_transport_buttons_and_assert_stack_remains(label: &str, winamp: Winamp) {
    let initial = window_geometry(winamp.stack);
    let sequence = [
        ("play", PLAY_X),
        ("pause", PAUSE_X),
        ("play", PLAY_X),
        ("pause", PAUSE_X),
        ("stop", STOP_X),
        ("play", PLAY_X),
        ("pause", PAUSE_X),
        ("stop", STOP_X),
    ];

    activate_window(winamp.stack);
    for (index, (button, x)) in sequence.into_iter().enumerate() {
        click_winamp_main_button(label, winamp, button, x, TRANSPORT_Y);
        std::thread::sleep(TRANSPORT_CLICK_SETTLE);
        assert_eq!(
            window_geometry(winamp.stack),
            initial,
            "{label} click {index} ({button}): the stack moved, resized or disappeared"
        );
    }
}

fn hold_transport_button_and_assert_pressed_until_release(label: &str, winamp: Winamp) {
    let initial = window_geometry(winamp.stack);
    activate_window(winamp.stack);

    let baseline = capture_winamp_button_crop(label, winamp, "baseline", PLAY_X, TRANSPORT_Y);
    let (screen_x, screen_y) = winamp_button_center(winamp, PLAY_X, TRANSPORT_Y);
    println!(
        "{label}: hold play window={} screen=({screen_x},{screen_y})",
        winamp.stack
    );
    mousemove_absolute(screen_x, screen_y);
    std::thread::sleep(Duration::from_millis(60));
    xdotool(["mousedown", "1"]);
    std::thread::sleep(BUTTON_HOLD_SETTLE);
    let pressed = capture_winamp_button_crop(label, winamp, "pressed", PLAY_X, TRANSPORT_Y);

    mousemove_absolute(screen_x + 1, screen_y);
    std::thread::sleep(BUTTON_HOLD_SAMPLE_DELAY);
    let held = capture_winamp_button_crop(label, winamp, "held", PLAY_X, TRANSPORT_Y);
    release_pointer(Duration::from_millis(180));
    let released = capture_winamp_button_crop(label, winamp, "released", PLAY_X, TRANSPORT_Y);

    let press_delta = image_changed_pixels(&baseline, &pressed, 8);
    let held_from_baseline = image_changed_pixels(&baseline, &held, 8);
    let held_from_pressed = image_changed_pixels(&pressed, &held, 8);
    let released_from_baseline = image_changed_pixels(&baseline, &released, 8);
    println!(
        "{label}: button crop deltas press={press_delta} held_from_baseline={held_from_baseline} held_from_pressed={held_from_pressed} released_from_baseline={released_from_baseline}"
    );

    assert!(
        press_delta >= 8,
        "{label}: play button did not enter a visible pressed state; baseline/pressed crop changed only {press_delta} pixels"
    );
    assert!(
        held_from_baseline >= press_delta.saturating_div(2).max(4),
        "{label}: play button returned to unpressed pixels before mouseup; press_delta={press_delta} held_from_baseline={held_from_baseline} held_from_pressed={held_from_pressed}"
    );
    assert!(
        held_from_pressed <= press_delta.saturating_div(2).max(6),
        "{label}: play button held crop drifted away from the pressed crop before mouseup; press_delta={press_delta} held_from_pressed={held_from_pressed}"
    );
    assert!(
        released_from_baseline <= press_delta.saturating_div(2).max(6),
        "{label}: play button stayed visually pressed after mouseup; press_delta={press_delta} released_from_baseline={released_from_baseline}"
    );

    assert_eq!(
        window_geometry(winamp.stack),
        initial,
        "{label}: the stack moved or resized"
    );
}

fn click_winamp_main_button(label: &str, winamp: Winamp, button: &str, x: f32, y: f32) {
    let (screen_x, screen_y) = winamp_button_center(winamp, x, y);
    println!(
        "{label}: click {button} window={} screen=({screen_x},{screen_y})",
        winamp.stack
    );
    mousemove_absolute(screen_x, screen_y);
    std::thread::sleep(Duration::from_millis(40));
    xdotool(["click", "1"]);
}

fn winamp_button_center(winamp: Winamp, x: f32, y: f32) -> (i32, i32) {
    let geometry = window_geometry(winamp.stack);
    (
        geometry.x + winamp.px(x + TRANSPORT_BUTTON_WIDTH * 0.5),
        geometry.y + winamp.px(y + TRANSPORT_BUTTON_HEIGHT * 0.5),
    )
}

fn diagnostic_png(label: &str, phase: &str) -> std::path::PathBuf {
    output_paths::diagnostic_path(&format!(
        "cranpose-winamp-{label}-{phase}-{}.png",
        std::process::id()
    ))
}

fn capture_winamp_button_crop(
    label: &str,
    winamp: Winamp,
    phase: &str,
    x: f32,
    y: f32,
) -> RgbaImage {
    let image = capture_x11_window_image(winamp.stack, &diagnostic_png(label, phase));
    crop_winamp_button(&image, winamp, x, y)
}

fn capture_x11_window_image(window_id: u64, path: &std::path::Path) -> RgbaImage {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", parent.display()));
    }
    let window_id_hex = format!("0x{window_id:x}");
    let path_text = path.to_string_lossy().into_owned();
    let status = Command::new("import")
        .args(["-silent", "-window", &window_id_hex, &path_text])
        .status()
        .unwrap_or_else(|err| panic!("import {window_id_hex} failed: {err}"));
    assert!(
        status.success(),
        "import failed for window {window_id_hex} -> {}",
        path.display()
    );
    image::open(path)
        .unwrap_or_else(|err| panic!("failed to open X11 capture {}: {err}", path.display()))
        .to_rgba8()
}

fn crop_winamp_button(image: &RgbaImage, winamp: Winamp, x: f32, y: f32) -> RgbaImage {
    let crop_x = (x * winamp.scale).floor().max(0.0) as u32;
    let crop_y = (y * winamp.scale).floor().max(0.0) as u32;
    let crop_w = (TRANSPORT_BUTTON_WIDTH * winamp.scale).ceil().max(1.0) as u32;
    let crop_h = (TRANSPORT_BUTTON_HEIGHT * winamp.scale).ceil().max(1.0) as u32;
    let crop_w = crop_w.min(image.width().saturating_sub(crop_x));
    let crop_h = crop_h.min(image.height().saturating_sub(crop_y));
    assert!(
        crop_w > 0 && crop_h > 0,
        "Winamp button crop outside captured image: image={}x{} crop=({}, {}, {}, {})",
        image.width(),
        image.height(),
        crop_x,
        crop_y,
        crop_w,
        crop_h
    );
    image::imageops::crop_imm(image, crop_x, crop_y, crop_w, crop_h).to_image()
}

fn image_changed_pixels(before: &RgbaImage, after: &RgbaImage, tolerance: u8) -> usize {
    assert_eq!(
        before.dimensions(),
        after.dimensions(),
        "image crop sizes differ"
    );
    before
        .pixels()
        .zip(after.pixels())
        .filter(|(left, right)| {
            left.0
                .iter()
                .zip(right.0.iter())
                .take(3)
                .any(|(a, b)| (*a).abs_diff(*b) > tolerance)
        })
        .count()
}

fn assert_windows_stop_after_release(label: &str, windows: &[u64]) {
    let expected: Vec<_> = windows.iter().map(|id| window_geometry(*id)).collect();
    let deadline = Instant::now() + POST_RELEASE_STABILITY_TIMEOUT;
    let mut sample = 0;
    while Instant::now() < deadline {
        sample += 1;
        let current: Vec<_> = windows.iter().map(|id| window_geometry(*id)).collect();
        assert!(
            current == expected,
            "{label} post-release sample {sample}: native windows kept moving after mouseup expected={expected:?} actual={current:?}"
        );
        std::thread::sleep(POST_RELEASE_STABILITY_POLL);
    }
}

fn assert_window_moved(label: &str, initial: WindowGeometry, final_geometry: WindowGeometry) {
    let dx = (final_geometry.x - initial.x).abs();
    let dy = (final_geometry.y - initial.y).abs();
    assert!(
        dx + dy >= 20,
        "{label}: dragged window did not move enough initial={initial:?} final={final_geometry:?}"
    );
}

fn window_geometry(window_id: u64) -> WindowGeometry {
    wmctrl_window_geometry(window_id)
        .or_else(|| xdotool_window_geometry(window_id))
        .unwrap_or_else(|| panic!("window manager geometry for window {window_id} not found"))
}

fn wmctrl_window_geometry(window_id: u64) -> Option<WindowGeometry> {
    let output = Command::new("wmctrl").arg("-lG").output().ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| {
            let parts: Vec<_> = line.split_whitespace().collect();
            if parts.len() < 6 {
                return None;
            }
            let id = u64::from_str_radix(parts[0].trim_start_matches("0x"), 16).ok()?;
            (id == window_id).then(|| WindowGeometry {
                x: parts[2].parse().expect("wmctrl X"),
                y: parts[3].parse().expect("wmctrl Y"),
                width: parts[4].parse().expect("wmctrl WIDTH"),
                height: parts[5].parse().expect("wmctrl HEIGHT"),
            })
        })
}

fn xdotool_window_geometry(window_id: u64) -> Option<WindowGeometry> {
    let output = Command::new("xdotool")
        .args(["getwindowgeometry", "--shell", &window_id.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let mut geometry = WindowGeometry {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "X" => geometry.x = value.parse().ok()?,
            "Y" => geometry.y = value.parse().ok()?,
            "WIDTH" => geometry.width = value.parse().ok()?,
            "HEIGHT" => geometry.height = value.parse().ok()?,
            _ => {}
        }
    }
    (geometry.width > 0 && geometry.height > 0).then_some(geometry)
}

fn window_title(window_id: u64) -> Option<String> {
    let output = Command::new("xdotool")
        .args(["getwindowname", &window_id.to_string()])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn visible_windows_summary() -> Vec<(u64, Option<String>, Option<WindowGeometry>)> {
    let output = Command::new("xdotool")
        .args(["search", "--onlyvisible", "--name", "."])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u64>().ok())
        .map(|id| (id, window_title(id), xdotool_window_geometry(id)))
        .collect()
}

fn move_window(window_id: u64, x: i32, y: i32) {
    let xdotool_window_id = window_id.to_string();
    xdotool([
        "windowmove",
        "--sync",
        &xdotool_window_id,
        &x.to_string(),
        &y.to_string(),
    ]);
    let Some(geometry) = xdotool_window_geometry(window_id) else {
        return;
    };
    if (geometry.x - x).abs() <= OFFSET_EPSILON && (geometry.y - y).abs() <= OFFSET_EPSILON {
        return;
    }
    move_window_with_wmctrl(window_id, x, y, geometry.width, geometry.height);
}

fn move_window_with_wmctrl(window_id: u64, x: i32, y: i32, width: i32, height: i32) {
    let window_id_hex = format!("0x{window_id:x}");
    let geometry = format!("0,{x},{y},{width},{height}");
    let status = Command::new("wmctrl")
        .args(["-ir", &window_id_hex, "-e", &geometry])
        .status();
    if !matches!(status, Ok(status) if status.success()) {
        println!("wmctrl move skipped for {window_id_hex}: {status:?}");
    }
}

fn activate_window(window_id: u64) {
    let status = Command::new("xdotool")
        .args(["windowactivate", &window_id.to_string()])
        .status();
    if !matches!(status, Ok(status) if status.success()) {
        println!("windowactivate skipped for {window_id}: {status:?}");
    }
    let status = Command::new("xdotool")
        .args(["windowraise", &window_id.to_string()])
        .status();
    if !matches!(status, Ok(status) if status.success()) {
        println!("windowraise skipped for {window_id}: {status:?}");
    }
    std::thread::sleep(Duration::from_millis(80));
}

fn mousemove_in_window_exact(label: &str, window_id: u64, x: i32, y: i32) {
    let geometry = window_geometry(window_id);
    let screen_x = geometry.x + x;
    let screen_y = geometry.y + y;
    for attempt in 0..3 {
        mousemove_absolute(screen_x, screen_y);
        std::thread::sleep(Duration::from_millis(15));
        let pointer = pointer_location();
        let dx = pointer.x - screen_x;
        let dy = pointer.y - screen_y;
        println!(
            "{label}: mouse target window={window_id} title={:?} geometry={geometry:?} desired_screen=({screen_x},{screen_y}) local=({x},{y}) attempt={} actual={pointer:?} actual_title={:?} error=({dx},{dy})",
            window_title(window_id),
            attempt + 1,
            window_title(pointer.window),
        );
        if dx.abs() <= 1 && dy.abs() <= 1 && pointer.window == window_id {
            return;
        }
    }

    panic!(
        "{label}: the pointer is not over window={window_id} title={:?} geometry={geometry:?} at local=({x},{y}) visible_windows={:?}",
        window_title(window_id),
        visible_windows_summary(),
    );
}

fn drag_pointer_by(dx: i32, dy: i32) {
    let pointer = pointer_location();
    mousemove_absolute(pointer.x + dx, pointer.y + dy);
}

fn mousemove_absolute(x: i32, y: i32) {
    xdotool(["mousemove", "--sync", "--", &x.to_string(), &y.to_string()]);
}

fn mousemove_absolute_unsynced(x: i32, y: i32) {
    xdotool(["mousemove", "--", &x.to_string(), &y.to_string()]);
}

fn pointer_location() -> PointerLocation {
    let output = Command::new("xdotool")
        .args(["getmouselocation", "--shell"])
        .output()
        .expect("xdotool getmouselocation");
    assert!(output.status.success(), "xdotool getmouselocation failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut location = PointerLocation {
        x: 0,
        y: 0,
        window: 0,
    };
    for line in stdout.lines() {
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "X" => location.x = value.parse().expect("mouse X"),
                "Y" => location.y = value.parse().expect("mouse Y"),
                "WINDOW" => location.window = value.parse().expect("mouse WINDOW"),
                _ => {}
            }
        }
    }
    location
}

fn click_button(robot: &cranpose::Robot, label: &str) {
    click_button_now(robot, label);
    std::thread::sleep(Duration::from_millis(250));
    let _ = robot.wait_for_idle();
}

fn click_button_now(robot: &cranpose::Robot, label: &str) {
    let (x, y, width, height) =
        find_button_in_semantics(robot, label).unwrap_or_else(|| panic!("{label} button"));
    robot
        .click(x + width * 0.5, y + height * 0.5)
        .unwrap_or_else(|err| panic!("click {label}: {err}"));
}

fn xdotool<const N: usize>(args: [&str; N]) {
    let status = Command::new("xdotool")
        .args(args)
        .status()
        .expect("xdotool");
    assert!(status.success(), "xdotool command failed");
}
