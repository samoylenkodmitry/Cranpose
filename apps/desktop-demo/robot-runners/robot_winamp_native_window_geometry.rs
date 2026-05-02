//! Robot test for native Winamp sub-window geometry.

use cranpose::AppLauncher;
use cranpose_testing::find_button_in_semantics;
use desktop_app::app::{self, DemoTab};
use std::collections::HashMap;
use std::process::Command;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const WINDOW_TITLE: &str = "Robot Winamp Native Geometry";
const MOVE_STEPS: usize = 12;
const FAST_MOVE_STEPS: usize = 8;
const FAST_MOVE_DX: i32 = 24;
const FAST_MOVE_DY: i32 = 11;
const PIXEL_TRACE_STEPS: usize = 48;
const PIXEL_TRACE_STALL_TIMEOUT: Duration = Duration::from_millis(90);
const GROUP_MOVE_STEP_TIMEOUT: Duration = Duration::from_millis(500);
const POST_RELEASE_STABILITY_TIMEOUT: Duration = Duration::from_millis(800);
const POST_RELEASE_STABILITY_POLL: Duration = Duration::from_millis(16);
const OFFSET_EPSILON: i32 = 3;
const FIND_WINDOW_POLL: Duration = Duration::from_millis(4);
const FIND_WINDOW_TIMEOUT: Duration = Duration::from_millis(12_000);
const SETUP_POSITION_TIMEOUT: Duration = Duration::from_millis(1200);
const CACHED_RESTORE_TIMEOUT: Duration = Duration::from_millis(1_000);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WindowGeometry {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PointerLocation {
    x: i32,
    y: i32,
    window: u64,
}

impl WindowGeometry {
    fn offset_from(self, other: Self) -> (i32, i32) {
        (self.x - other.x, self.y - other.y)
    }
}

#[derive(Clone, Copy, Debug)]
struct WinampWindows {
    main: u64,
    equalizer: u64,
    playlist: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WinampGeometries {
    main: WindowGeometry,
    equalizer: WindowGeometry,
    playlist: WindowGeometry,
}

#[derive(Clone, Copy, Debug)]
struct DragTraceSample {
    pointer: PointerLocation,
    geometries: WinampGeometries,
    elapsed: Duration,
}

impl WinampWindows {
    fn geometries(self) -> WinampGeometries {
        let geometries = window_geometries(self.window_ids());
        WinampGeometries {
            main: geometry_from_snapshot(&geometries, self.main),
            equalizer: geometry_from_snapshot(&geometries, self.equalizer),
            playlist: geometry_from_snapshot(&geometries, self.playlist),
        }
    }

    fn window_ids(self) -> [u64; 3] {
        [self.main, self.equalizer, self.playlist]
    }
}

fn main() {
    env_logger::init();
    println!("=== Robot Winamp Native Window Geometry ===");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(1200, 800)
        .with_headless(false)
        .with_test_driver(|robot| {
            let pid = std::process::id();
            let host_window = find_app_window(pid);
            let host_origin = host_window_origin();
            move_window(host_window, host_origin.x, host_origin.y);
            std::thread::sleep(Duration::from_millis(250));
            robot.wait_for_idle().expect("host window positioned");

            let appeared_at = Instant::now();
            click_button_now(&robot, "Winamp");
            let windows = find_winamp_windows(pid);
            let appearance_elapsed = appeared_at.elapsed();
            robot.wait_for_idle().expect("initial Winamp idle");
            std::thread::sleep(Duration::from_millis(500));
            robot
                .wait_for_idle()
                .expect("initial native windows settled");
            println!(
                "native windows appeared in {}ms: {:?}",
                appearance_elapsed.as_millis(),
                windows
            );
            println!("appeared: {:?}", windows.geometries());
            let origin = arrange_origin(windows);

            place_attached_chain(windows, origin.x, origin.y);
            assert_attached_offsets("arranged", windows.geometries());

            drag_main_and_assert_offsets("drag-main", windows);
            place_attached_chain(windows, origin.x + 10, origin.y + 5);
            drag_main_one_pixel_trace_and_assert_continuity("drag-main-pixel-trace", windows);
            place_attached_chain(windows, origin.x + 20, origin.y + 10);
            drag_main_fast_and_assert_offsets("drag-main-fast", windows);
            place_attached_chain(windows, origin.x + 40, origin.y + 20);
            drag_and_assert_only_dragged("drag-playlist", windows, windows.playlist);
            place_attached_chain(windows, origin.x + 80, origin.y + 40);
            drag_and_assert_only_dragged("drag-equalizer", windows, windows.equalizer);
            place_attached_chain(windows, origin.x + 100, origin.y + 50);
            move_main_with_window_manager_and_assert_offsets("wm-move-main", windows, 31, 19);

            click_button(&robot, "Dock");
            assert_windows_absent(pid, "after Dock");

            let restore_started = Instant::now();
            click_button_now(&robot, "Undock");
            let restored_windows = find_winamp_windows(pid);
            let restore_elapsed = restore_started.elapsed();
            println!(
                "native windows restored after Undock in {}ms: {:?}",
                restore_elapsed.as_millis(),
                restored_windows
            );
            assert_eq!(
                windows.window_ids(),
                restored_windows.window_ids(),
                "Undock recreated native windows instead of restoring cached OS windows"
            );
            assert!(
                restore_elapsed <= CACHED_RESTORE_TIMEOUT,
                "Undock restore took {}ms, expected cached native windows to return within {}ms",
                restore_elapsed.as_millis(),
                CACHED_RESTORE_TIMEOUT.as_millis()
            );
            click_button(&robot, "Counter App");
            assert_windows_absent(pid, "after tab switch");

            robot.exit().expect("exit");
        })
        .run(|| app::combined_app_with_initial_tab(Some(DemoTab::Counter)));
}

fn arrange_origin(windows: WinampWindows) -> WindowGeometry {
    let geometries = windows.geometries();
    let total_width = geometries.main.width + geometries.playlist.width;
    let total_height =
        geometries.main.height + geometries.equalizer.height.max(geometries.playlist.height);
    let desired_margin = 120;
    let monitor = *monitor_rects()
        .iter()
        .max_by_key(|monitor| monitor.width * monitor.height)
        .expect("at least one monitor");
    let margin_x = desired_margin.min(((monitor.width - total_width).max(0) / 2).max(8));
    let margin_y = desired_margin.min(((monitor.height - total_height).max(0) / 2).max(8));
    let max_x = monitor.x + monitor.width - total_width - margin_x;
    let max_y = monitor.y + monitor.height - total_height - margin_y;
    WindowGeometry {
        x: (monitor.x + margin_x).min(max_x.max(monitor.x)),
        y: (monitor.y + margin_y).min(max_y.max(monitor.y)),
        width: geometries.main.width,
        height: geometries.main.height,
    }
}

fn host_window_origin() -> WindowGeometry {
    let monitor = *monitor_rects()
        .iter()
        .max_by_key(|monitor| monitor.width * monitor.height)
        .expect("at least one monitor");
    WindowGeometry {
        x: monitor.x + 160,
        y: monitor.y + 120,
        width: 1200,
        height: 800,
    }
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

fn find_winamp_windows(pid: u32) -> WinampWindows {
    let deadline = Instant::now() + FIND_WINDOW_TIMEOUT;
    while Instant::now() < deadline {
        let ids = find_window_ids_by_title(pid);
        if let (Some(main), Some(equalizer), Some(playlist)) = (
            ids.get("Winamp").copied(),
            ids.get("Winamp Equalizer").copied(),
            ids.get("Winamp Playlist").copied(),
        ) {
            return WinampWindows {
                main,
                equalizer,
                playlist,
            };
        }
        if let Some(windows) = find_winamp_windows_from_visible_summary() {
            return windows;
        }
        std::thread::sleep(FIND_WINDOW_POLL);
    }

    panic!(
        "Winamp native windows for pid {pid} not found; visible windows: {:?}",
        visible_windows_summary()
    );
}

fn find_winamp_windows_from_visible_summary() -> Option<WinampWindows> {
    let mut ids = HashMap::new();
    for (id, title, geometry) in visible_windows_summary() {
        let Some(title) = title else {
            continue;
        };
        if !matches!(
            title.as_str(),
            "Winamp" | "Winamp Equalizer" | "Winamp Playlist"
        ) {
            continue;
        }
        if geometry.is_some() {
            ids.insert(title, id);
        }
    }

    Some(WinampWindows {
        main: ids.get("Winamp").copied()?,
        equalizer: ids.get("Winamp Equalizer").copied()?,
        playlist: ids.get("Winamp Playlist").copied()?,
    })
}

fn find_window_id_by_exact_title(pid: u32, title: &str) -> Option<u64> {
    if let Some(mut windows) = wmctrl_window_ids_by_title(pid) {
        if let Some(id) = windows.remove(title) {
            return Some(id);
        }
    }

    find_window_ids_by_exact_title(pid, title)
        .into_iter()
        .next()
}

fn find_window_ids(pid: u32, title: &str) -> Vec<u64> {
    find_window_ids_by_exact_title(pid, title)
}

fn find_window_ids_by_title(pid: u32) -> HashMap<String, u64> {
    if let Some(windows) = wmctrl_window_ids_by_title(pid) {
        let windows: HashMap<_, _> = windows
            .into_iter()
            .filter(|(title, id)| {
                matches!(
                    title.as_str(),
                    "Winamp" | "Winamp Equalizer" | "Winamp Playlist"
                ) && window_geometries([*id]).contains_key(id)
            })
            .collect();
        if !windows.is_empty() {
            return windows;
        }
    }

    let mut windows = HashMap::new();
    for title in ["Winamp", "Winamp Equalizer", "Winamp Playlist"] {
        if let Some(id) = find_window_ids_by_exact_title(pid, title)
            .into_iter()
            .next()
        {
            windows.insert(title.to_string(), id);
        }
    }
    windows
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

fn find_window_ids_by_exact_title(pid: u32, title: &str) -> Vec<u64> {
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

fn monitor_rects() -> &'static [WindowGeometry] {
    static MONITORS: OnceLock<Vec<WindowGeometry>> = OnceLock::new();
    MONITORS.get_or_init(|| {
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
    })
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
        let ids = [
            find_window_ids(pid, "Winamp"),
            find_window_ids(pid, "Winamp Equalizer"),
            find_window_ids(pid, "Winamp Playlist"),
        ];
        if ids.iter().all(Vec::is_empty) {
            println!("{label}: native windows absent");
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let windows = find_window_ids_by_title(pid);
    let geometries: Vec<_> = windows
        .iter()
        .map(|(title, id)| (title.clone(), *id, window_geometry(*id)))
        .collect();
    panic!("{label}: native Winamp windows are still visible for pid {pid}: {geometries:?}");
}

fn place_attached_chain(windows: WinampWindows, x: i32, y: i32) {
    let sizes = windows.geometries();
    place_window_for_setup("place-chain-main", windows.main, x, y);
    place_window_for_setup(
        "place-chain-equalizer",
        windows.equalizer,
        x,
        y + sizes.main.height,
    );
    place_window_for_setup(
        "place-chain-playlist",
        windows.playlist,
        x + sizes.equalizer.width,
        y + sizes.main.height,
    );
    std::thread::sleep(Duration::from_millis(220));
    println!("place attached chain: {:?}", windows.geometries());
}

fn place_window_for_setup(label: &str, window_id: u64, x: i32, y: i32) {
    activate_window(window_id);
    move_window(window_id, x, y);
    let mut last_move_requested = Instant::now();
    let deadline = Instant::now() + SETUP_POSITION_TIMEOUT;
    while Instant::now() < deadline {
        let geometry = window_geometry(window_id);
        if (geometry.x - x).abs() <= OFFSET_EPSILON && (geometry.y - y).abs() <= OFFSET_EPSILON {
            std::thread::sleep(Duration::from_millis(180));
            let stable = window_geometry(window_id);
            if (stable.x - x).abs() <= OFFSET_EPSILON && (stable.y - y).abs() <= OFFSET_EPSILON {
                return;
            }
        }
        if last_move_requested.elapsed() >= Duration::from_millis(50) {
            move_window(window_id, x, y);
            last_move_requested = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(8));
    }

    panic!(
        "{label}: OS window did not reach setup position target=({x},{y}) actual={:?}",
        window_geometry(window_id)
    );
}

fn drag_main_and_assert_offsets(label: &str, windows: WinampWindows) {
    drag_and_assert_offsets(label, windows, windows.main, true);
}

fn drag_main_one_pixel_trace_and_assert_continuity(label: &str, windows: WinampWindows) {
    let initial = assert_attached_offsets(label, windows.geometries());
    let (start_x, start_y) = drag_start_for_window(windows, windows.main);

    activate_window(windows.main);
    mousemove_in_window_exact(label, windows.main, start_x, start_y);
    std::thread::sleep(Duration::from_millis(100));
    assert_pointer_over_window(label, windows.main);
    xdotool(["mousedown", "1"]);
    std::thread::sleep(Duration::from_millis(60));

    let mut trace = Vec::with_capacity(PIXEL_TRACE_STEPS + 1);
    trace.push(DragTraceSample {
        pointer: pointer_location(),
        geometries: windows.geometries(),
        elapsed: Duration::ZERO,
    });

    for step in 1..=PIXEL_TRACE_STEPS {
        let previous = *trace.last().expect("previous trace sample");
        mousemove_relative(1, 0);
        trace.push(wait_for_one_pixel_drag_step(label, step, windows, previous));
        std::thread::sleep(Duration::from_millis(8));
    }

    xdotool(["mouseup", "1"]);
    std::thread::sleep(Duration::from_millis(120));
    print_drag_trace(label, &trace);
    assert_pixel_drag_trace_continuity(label, initial, &trace);
    assert_windows_stop_after_release(label, windows, windows.geometries());
}

fn drag_main_fast_and_assert_offsets(label: &str, windows: WinampWindows) {
    let initial = assert_attached_offsets(label, windows.geometries());
    let (start_x, start_y) = drag_start_for_window(windows, windows.main);

    activate_window(windows.main);
    mousemove_in_window_exact(label, windows.main, start_x, start_y);
    std::thread::sleep(Duration::from_millis(100));
    assert_pointer_over_window(label, windows.main);
    xdotool(["mousedown", "1"]);
    std::thread::sleep(Duration::from_millis(60));

    let mut previous_main = initial.main;
    for step in 1..=FAST_MOVE_STEPS {
        let moved_at = Instant::now();
        mousemove_relative(FAST_MOVE_DX, FAST_MOVE_DY);
        let current = wait_for_group_offset(
            label,
            step,
            windows,
            initial,
            previous_main,
            GROUP_MOVE_STEP_TIMEOUT,
        );
        previous_main = current.main;
        println!(
            "{label} step {step}: followed in {}ms {:?}",
            moved_at.elapsed().as_millis(),
            current
        );
    }

    xdotool(["mouseup", "1"]);
    std::thread::sleep(Duration::from_millis(120));
    let final_geometries = windows.geometries();
    assert_offsets_close(label, FAST_MOVE_STEPS + 1, initial, final_geometries);
    assert_window_moved(label, initial.main, final_geometries.main);
    assert_windows_stop_after_release(label, windows, final_geometries);
}

fn print_drag_trace(label: &str, trace: &[DragTraceSample]) {
    for (index, sample) in trace.iter().enumerate() {
        println!(
            "{label} trace {index:02}: dt={}ms pointer=({}, {}) main=({}, {}) eq=({}, {}) pl=({}, {})",
            sample.elapsed.as_millis(),
            sample.pointer.x,
            sample.pointer.y,
            sample.geometries.main.x,
            sample.geometries.main.y,
            sample.geometries.equalizer.x,
            sample.geometries.equalizer.y,
            sample.geometries.playlist.x,
            sample.geometries.playlist.y,
        );
    }
}

fn wait_for_one_pixel_drag_step(
    label: &str,
    step: usize,
    windows: WinampWindows,
    previous: DragTraceSample,
) -> DragTraceSample {
    let started = Instant::now();
    loop {
        let sample = DragTraceSample {
            pointer: pointer_location(),
            geometries: windows.geometries(),
            elapsed: started.elapsed(),
        };
        let pointer_dx = sample.pointer.x - previous.pointer.x;
        let pointer_dy = sample.pointer.y - previous.pointer.y;
        let main_dx = sample.geometries.main.x - previous.geometries.main.x;
        let main_dy = sample.geometries.main.y - previous.geometries.main.y;
        let equalizer_dx = sample.geometries.equalizer.x - previous.geometries.equalizer.x;
        let equalizer_dy = sample.geometries.equalizer.y - previous.geometries.equalizer.y;
        let playlist_dx = sample.geometries.playlist.x - previous.geometries.playlist.x;
        let playlist_dy = sample.geometries.playlist.y - previous.geometries.playlist.y;

        if pointer_dx > 1 || pointer_dy != 0 {
            panic!(
                "{label} step {step}: robot pointer moved incorrectly previous={previous:?} sample={sample:?}"
            );
        }
        if main_dx > 1
            || main_dy != 0
            || equalizer_dx > 1
            || equalizer_dy != 0
            || playlist_dx > 1
            || playlist_dy != 0
        {
            panic!(
                "{label} step {step}: staircase jump detected previous={previous:?} sample={sample:?}"
            );
        }
        if pointer_dx == 1
            && main_dx == 1
            && equalizer_dx == 1
            && playlist_dx == 1
            && main_dy == 0
            && equalizer_dy == 0
            && playlist_dy == 0
        {
            return sample;
        }
        if sample.elapsed > PIXEL_TRACE_STALL_TIMEOUT {
            panic!(
                "{label} step {step}: one-pixel drag did not complete within {}ms previous={previous:?} last={sample:?}",
                PIXEL_TRACE_STALL_TIMEOUT.as_millis()
            );
        }

        std::thread::sleep(Duration::from_millis(1));
    }
}

fn assert_one_pixel_window_step(
    label: &str,
    index: usize,
    window: &str,
    previous: WindowGeometry,
    current: WindowGeometry,
) {
    let dx = current.x - previous.x;
    let dy = current.y - previous.y;
    assert_eq!(
        dx, 1,
        "{label} sample {index}: {window} did not follow the one-pixel pointer step exactly"
    );
    assert_eq!(
        dy, 0,
        "{label} sample {index}: {window} moved vertically during horizontal trace"
    );
}

fn assert_pixel_drag_trace_continuity(
    label: &str,
    expected_offsets: WinampGeometries,
    trace: &[DragTraceSample],
) {
    assert!(
        trace.len() == PIXEL_TRACE_STEPS + 1,
        "{label}: expected {} trace samples, got {}",
        PIXEL_TRACE_STEPS + 1,
        trace.len()
    );

    for index in 1..trace.len() {
        let previous = trace[index - 1].geometries;
        let current = trace[index].geometries;
        let pointer_dx = trace[index].pointer.x - trace[index - 1].pointer.x;
        let pointer_dy = trace[index].pointer.y - trace[index - 1].pointer.y;

        assert_eq!(
            pointer_dx, 1,
            "{label} sample {index}: robot pointer did not advance by exactly one pixel trace={trace:?}"
        );
        assert_eq!(
            pointer_dy, 0,
            "{label} sample {index}: robot pointer moved vertically during horizontal trace trace={trace:?}"
        );
        assert_offsets_close(label, index, expected_offsets, current);
        assert_one_pixel_window_step(label, index, "main", previous.main, current.main);
        assert_one_pixel_window_step(
            label,
            index,
            "equalizer",
            previous.equalizer,
            current.equalizer,
        );
        assert_one_pixel_window_step(
            label,
            index,
            "playlist",
            previous.playlist,
            current.playlist,
        );
    }

    let first = trace.first().expect("first trace sample").geometries.main;
    let last = trace.last().expect("last trace sample").geometries.main;
    let total_dx = last.x - first.x;
    assert_eq!(
        total_dx, PIXEL_TRACE_STEPS as i32,
        "{label}: total main movement should match pointer pixels expected={} actual={total_dx} trace={trace:?}",
        PIXEL_TRACE_STEPS
    );
}

fn drag_and_assert_only_dragged(label: &str, windows: WinampWindows, dragged_window: u64) {
    drag_and_assert_offsets(label, windows, dragged_window, false);
}

fn drag_and_assert_offsets(
    label: &str,
    windows: WinampWindows,
    dragged_window: u64,
    moves_attached_group: bool,
) {
    let initial = assert_attached_offsets(label, windows.geometries());
    let initial_dragged = geometry_for_window(windows, dragged_window, initial);
    let (start_x, start_y) = drag_start_for_window(windows, dragged_window);

    activate_window(dragged_window);
    mousemove_in_window_exact(label, dragged_window, start_x, start_y);
    std::thread::sleep(Duration::from_millis(100));
    assert_pointer_over_window(label, dragged_window);
    xdotool(["mousedown", "1"]);
    std::thread::sleep(Duration::from_millis(60));

    let mut previous_group_main = initial.main;
    for step in 1..=MOVE_STEPS {
        mousemove_relative(13, 7);
        let current = if moves_attached_group {
            let current = wait_for_group_offset(
                label,
                step,
                windows,
                initial,
                previous_group_main,
                GROUP_MOVE_STEP_TIMEOUT,
            );
            previous_group_main = current.main;
            current
        } else {
            std::thread::sleep(Duration::from_millis(45));
            windows.geometries()
        };
        println!("{label} step {step}: {:?}", current);
        if moves_attached_group {
            assert_offsets_close(label, step, initial, current);
        } else {
            assert_only_dragged_window_changed(
                label,
                step,
                windows,
                dragged_window,
                initial,
                current,
            );
        }
    }

    xdotool(["mouseup", "1"]);
    std::thread::sleep(Duration::from_millis(120));
    let final_geometries = windows.geometries();
    if moves_attached_group {
        assert_offsets_close(label, MOVE_STEPS + 1, initial, final_geometries);
    } else {
        assert_only_dragged_window_changed(
            label,
            MOVE_STEPS + 1,
            windows,
            dragged_window,
            initial,
            final_geometries,
        );
    }
    let final_dragged = geometry_for_window(windows, dragged_window, final_geometries);
    assert_window_moved(label, initial_dragged, final_dragged);
    assert_windows_stop_after_release(label, windows, final_geometries);
}

fn move_main_with_window_manager_and_assert_offsets(
    label: &str,
    windows: WinampWindows,
    dx: i32,
    dy: i32,
) {
    if !window_manager_supports_net_active_window() {
        println!("{label}: skipping external window-manager move; _NET_ACTIVE_WINDOW unsupported");
        return;
    }

    let initial = assert_attached_offsets(label, windows.geometries());
    move_window(windows.main, initial.main.x + dx, initial.main.y + dy);

    let deadline = Instant::now() + SETUP_POSITION_TIMEOUT;
    while Instant::now() < deadline {
        let current = windows.geometries();
        if (current.main.x - (initial.main.x + dx)).abs() <= OFFSET_EPSILON
            && (current.main.y - (initial.main.y + dy)).abs() <= OFFSET_EPSILON
            && offsets_close(initial, current)
        {
            println!("{label}: {:?}", current);
            assert_windows_stop_after_release(label, windows, current);
            return;
        }
        std::thread::sleep(Duration::from_millis(8));
    }

    let current = windows.geometries();
    assert_offsets_close(label, 1, initial, current);
    assert_window_moved(label, initial.main, current.main);
    assert_windows_stop_after_release(label, windows, current);
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

fn wait_for_group_offset(
    label: &str,
    step: usize,
    windows: WinampWindows,
    expected: WinampGeometries,
    previous_main: WindowGeometry,
    timeout: Duration,
) -> WinampGeometries {
    let deadline = Instant::now() + timeout;
    let mut current = windows.geometries();
    while Instant::now() < deadline {
        current = windows.geometries();
        if offsets_close(expected, current) && current.main != previous_main {
            return current;
        }
        std::thread::sleep(Duration::from_millis(4));
    }
    assert_offsets_close(label, step, expected, current);
    assert_window_moved(label, previous_main, current.main);
    current
}

fn assert_windows_stop_after_release(
    label: &str,
    windows: WinampWindows,
    expected: WinampGeometries,
) {
    let deadline = Instant::now() + POST_RELEASE_STABILITY_TIMEOUT;
    let mut sample = 0;
    let mut samples = Vec::new();
    while Instant::now() < deadline {
        sample += 1;
        let current = windows.geometries();
        samples.push(current);
        if current != expected {
            panic!(
                "{label} post-release sample {sample}: native windows kept moving after mouseup expected={expected:?} actual={current:?} samples={samples:?}"
            );
        }
        std::thread::sleep(POST_RELEASE_STABILITY_POLL);
    }
}

fn drag_start_for_window(windows: WinampWindows, window_id: u64) -> (i32, i32) {
    if window_id == windows.main {
        (120, 8)
    } else {
        (32, 12)
    }
}

fn geometry_for_window(
    windows: WinampWindows,
    window_id: u64,
    geometries: WinampGeometries,
) -> WindowGeometry {
    if window_id == windows.main {
        geometries.main
    } else if window_id == windows.equalizer {
        geometries.equalizer
    } else if window_id == windows.playlist {
        geometries.playlist
    } else {
        panic!("unknown Winamp window id {window_id}");
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

fn assert_only_dragged_window_changed(
    label: &str,
    step: usize,
    windows: WinampWindows,
    dragged_window: u64,
    expected: WinampGeometries,
    actual: WinampGeometries,
) {
    if dragged_window != windows.main {
        assert_geometry_close(label, step, "main", expected.main, actual.main);
    }
    if dragged_window != windows.equalizer {
        assert_geometry_close(
            label,
            step,
            "equalizer",
            expected.equalizer,
            actual.equalizer,
        );
    }
    if dragged_window != windows.playlist {
        assert_geometry_close(label, step, "playlist", expected.playlist, actual.playlist);
    }
}

fn assert_geometry_close(
    label: &str,
    step: usize,
    window: &str,
    expected: WindowGeometry,
    actual: WindowGeometry,
) {
    let dx = (actual.x - expected.x).abs();
    let dy = (actual.y - expected.y).abs();
    assert!(
        dx <= OFFSET_EPSILON && dy <= OFFSET_EPSILON,
        "{label} step {step}: {window} moved while dragging another window expected={expected:?} actual={actual:?} delta=({dx},{dy})"
    );
}

fn assert_attached_offsets(label: &str, geometries: WinampGeometries) -> WinampGeometries {
    println!("{label}: {:?}", geometries);
    assert_pair_attached(label, geometries.main, geometries.equalizer);
    assert_pair_attached(label, geometries.equalizer, geometries.playlist);
    geometries
}

fn assert_offsets_close(
    label: &str,
    step: usize,
    expected: WinampGeometries,
    actual: WinampGeometries,
) {
    assert_offset_close(
        label,
        step,
        "equalizer-main",
        expected.equalizer.offset_from(expected.main),
        actual.equalizer.offset_from(actual.main),
    );
    assert_offset_close(
        label,
        step,
        "playlist-equalizer",
        expected.playlist.offset_from(expected.equalizer),
        actual.playlist.offset_from(actual.equalizer),
    );
}

fn offsets_close(expected: WinampGeometries, actual: WinampGeometries) -> bool {
    offset_close(
        expected.equalizer.offset_from(expected.main),
        actual.equalizer.offset_from(actual.main),
    ) && offset_close(
        expected.playlist.offset_from(expected.equalizer),
        actual.playlist.offset_from(actual.equalizer),
    )
}

fn offset_close(expected: (i32, i32), actual: (i32, i32)) -> bool {
    let dx = (actual.0 - expected.0).abs();
    let dy = (actual.1 - expected.1).abs();
    dx <= OFFSET_EPSILON && dy <= OFFSET_EPSILON
}

fn assert_offset_close(
    label: &str,
    step: usize,
    edge: &str,
    expected: (i32, i32),
    actual: (i32, i32),
) {
    let dx = (actual.0 - expected.0).abs();
    let dy = (actual.1 - expected.1).abs();
    assert!(
        dx <= OFFSET_EPSILON && dy <= OFFSET_EPSILON,
        "{label} step {step}: {edge} offset stretched expected={expected:?} actual={actual:?} delta=({dx},{dy})"
    );
}

fn assert_pair_attached(label: &str, first: WindowGeometry, second: WindowGeometry) {
    let first_right = first.x + first.width;
    let first_bottom = first.y + first.height;
    let second_right = second.x + second.width;
    let second_bottom = second.y + second.height;

    let touches_horizontal = (first_right - second.x).abs() <= OFFSET_EPSILON
        || (second_right - first.x).abs() <= OFFSET_EPSILON;
    let overlaps_vertical =
        first.y <= second_bottom + OFFSET_EPSILON && second.y <= first_bottom + OFFSET_EPSILON;
    let touches_vertical = (first_bottom - second.y).abs() <= OFFSET_EPSILON
        || (second_bottom - first.y).abs() <= OFFSET_EPSILON;
    let overlaps_horizontal =
        first.x <= second_right + OFFSET_EPSILON && second.x <= first_right + OFFSET_EPSILON;

    assert!(
        touches_horizontal && overlaps_vertical || touches_vertical && overlaps_horizontal,
        "{label}: windows are not attached first={first:?} second={second:?}"
    );
}

fn window_geometry(window_id: u64) -> WindowGeometry {
    let mut geometries = window_geometries([window_id]);
    geometries
        .remove(&window_id)
        .unwrap_or_else(|| panic!("window manager geometry for window {window_id} not found"))
}

fn window_geometries<const N: usize>(window_ids: [u64; N]) -> HashMap<u64, WindowGeometry> {
    let wmctrl = wmctrl_window_geometries(window_ids);
    if wmctrl.len() == window_ids.len() {
        return wmctrl;
    }

    let mut geometries = HashMap::new();
    for id in window_ids {
        if let Some(geometry) = xdotool_window_geometry(id) {
            geometries.insert(id, geometry);
        }
    }
    geometries
}

fn wmctrl_window_geometries<const N: usize>(window_ids: [u64; N]) -> HashMap<u64, WindowGeometry> {
    let Some(output) = Command::new("wmctrl").arg("-lG").output().ok() else {
        return HashMap::new();
    };
    if !output.status.success() {
        return HashMap::new();
    }

    let mut geometries = HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let parts: Vec<_> = line.split_whitespace().collect();
        if parts.len() < 6 {
            continue;
        }
        let Some(id) = parts
            .first()
            .and_then(|part| u64::from_str_radix(part.trim_start_matches("0x"), 16).ok())
        else {
            continue;
        };
        if window_ids.contains(&id) {
            geometries.insert(
                id,
                WindowGeometry {
                    x: parts[2].parse().expect("wmctrl X"),
                    y: parts[3].parse().expect("wmctrl Y"),
                    width: parts[4].parse().expect("wmctrl WIDTH"),
                    height: parts[5].parse().expect("wmctrl HEIGHT"),
                },
            );
        }
    }
    geometries
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

fn geometry_from_snapshot(
    geometries: &HashMap<u64, WindowGeometry>,
    window_id: u64,
) -> WindowGeometry {
    *geometries
        .get(&window_id)
        .unwrap_or_else(|| panic!("window manager geometry for window {window_id} not found"))
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
}

fn activate_window(window_id: u64) {
    let status = Command::new("xdotool")
        .args(["windowactivate", &window_id.to_string()])
        .status();
    if !matches!(status, Ok(status) if status.success()) {
        println!("windowactivate skipped for {window_id}: {status:?}");
    }
    std::thread::sleep(Duration::from_millis(80));
}

fn mousemove_in_window_exact(label: &str, window_id: u64, x: i32, y: i32) {
    let geometry = window_geometry(window_id);
    let screen_x = geometry.x + x;
    let screen_y = geometry.y + y;
    for attempt in 0..3 {
        xdotool([
            "mousemove",
            "--sync",
            "--",
            &screen_x.to_string(),
            &screen_y.to_string(),
        ]);
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
        if dx.abs() <= 1 && dy.abs() <= 1 {
            return;
        }
    }
}

fn assert_pointer_over_window(label: &str, expected_window: u64) {
    let pointer = pointer_location();
    assert_eq!(
        pointer.window,
        expected_window,
        "{label}: pointer is over the wrong OS window expected={expected_window} expected_title={:?} actual={pointer:?} actual_title={:?}",
        window_title(expected_window),
        window_title(pointer.window),
    );
}

fn mousemove_relative(dx: i32, dy: i32) {
    xdotool(["mousemove_relative", "--", &dx.to_string(), &dy.to_string()]);
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
