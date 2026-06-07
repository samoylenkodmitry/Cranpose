use cranpose_testing::{
    find_button_exact_in_semantics, find_button_in_semantics, find_in_semantics, find_text_exact,
    find_text_in_semantics,
};
use image::RgbaImage;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

#[allow(dead_code)]
pub(crate) fn find_window_id(title: &str) -> String {
    let process_id = std::process::id().to_string();
    for _ in 0..20 {
        if let Some(id) = find_visible_process_window_id(title, &process_id) {
            return id;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("window '{title}' not found via xdotool");
}

#[allow(dead_code)]
pub(crate) fn take_x11_screenshot(window_id: &str, path: &str) {
    if let Some(parent) = Path::new(path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    for attempt in 1..=8 {
        prepare_x11_window_for_capture(window_id);

        if take_x11_screenshot_with_import(window_id, path) {
            return;
        }
        eprintln!("import capture attempt {attempt} failed for window {window_id} -> {path}");

        if take_x11_screenshot_with_xwd(window_id, path) {
            return;
        }
        eprintln!("xwd capture attempt {attempt} failed for window {window_id} -> {path}");

        std::thread::sleep(Duration::from_millis(120 + attempt * 40));
    }

    assert!(
        file_has_bytes(path),
        "X11 screenshot failed for window {window_id} -> {path} after import/xwd retries"
    );
}

#[allow(dead_code)]
pub(crate) fn capture_x11_window(window_id: &str, path: &Path) -> RgbaImage {
    let path_text = path.to_string_lossy().into_owned();
    take_x11_screenshot(window_id, &path_text);
    image::open(path)
        .unwrap_or_else(|err| panic!("failed to open X11 capture {}: {err}", path.display()))
        .to_rgba8()
}

#[allow(dead_code)]
pub(crate) fn focus_x11_window(window_id: &str) {
    let _ = Command::new("xdotool")
        .args(["windowactivate", "--sync", window_id])
        .status();
    let _ = Command::new("xdotool")
        .args(["windowraise", window_id])
        .status();
    std::thread::sleep(Duration::from_millis(80));
}

#[allow(dead_code)]
pub(crate) fn move_x11_mouse(x: f32, y: f32) {
    let x = x.round().to_string();
    let y = y.round().to_string();
    let status = Command::new("xdotool")
        .args(["mousemove", "--sync", "--", &x, &y])
        .status()
        .expect("xdotool mousemove");
    assert!(status.success(), "xdotool mousemove failed");
}

#[allow(dead_code)]
pub(crate) fn move_x11_mouse_in_window(window_id: &str, x: f32, y: f32) {
    let x = x.round().to_string();
    let y = y.round().to_string();
    let status = Command::new("xdotool")
        .args(["mousemove", "--sync", "--window", window_id, "--", &x, &y])
        .status()
        .expect("xdotool window-relative mousemove");
    assert!(
        status.success(),
        "xdotool window-relative mousemove failed for {window_id}"
    );
}

#[allow(dead_code)]
pub(crate) fn click_x11_button(button: &str) {
    let status = Command::new("xdotool")
        .args(["click", button])
        .status()
        .expect("xdotool click");
    assert!(status.success(), "xdotool click {button} failed");
}

#[allow(dead_code)]
pub(crate) fn mouse_down_x11_button(button: &str) {
    let status = Command::new("xdotool")
        .args(["mousedown", button])
        .status()
        .expect("xdotool mousedown");
    assert!(status.success(), "xdotool mousedown {button} failed");
}

#[allow(dead_code)]
pub(crate) fn mouse_up_x11_button(button: &str) {
    let status = Command::new("xdotool")
        .args(["mouseup", button])
        .status()
        .expect("xdotool mouseup");
    assert!(status.success(), "xdotool mouseup {button} failed");
}

#[allow(dead_code)]
pub(crate) fn click_x11_button_repeated(button: &str, count: usize) {
    let count = count.to_string();
    let status = Command::new("xdotool")
        .args(["click", "--repeat", &count, "--delay", "0", button])
        .status()
        .expect("xdotool repeated click");
    assert!(
        status.success(),
        "xdotool repeated click {button} x{count} failed"
    );
}

#[allow(dead_code)]
pub(crate) fn capture_x11_window_screenshot(
    window_id: &str,
    path: &Path,
    logical_width: f32,
    logical_height: f32,
) -> cranpose::RobotScreenshot {
    let image = capture_x11_window(window_id, path);
    let (width, height) = image.dimensions();
    cranpose::RobotScreenshot {
        width,
        height,
        logical_width,
        logical_height,
        pixels: image.into_raw(),
    }
}

fn take_x11_screenshot_with_xwd(window_id: &str, path: &str) -> bool {
    let xwd_path = Path::new(path).with_extension(format!("{}.xwd", std::process::id()));
    let xwd_target = xwd_path.to_string_lossy().into_owned();
    let xwd_status = Command::new("xwd")
        .args(["-silent", "-id", window_id, "-out", &xwd_target])
        .status();
    if !matches!(xwd_status, Ok(status) if status.success()) || !file_has_bytes(&xwd_target) {
        let _ = std::fs::remove_file(&xwd_path);
        return false;
    }

    let convert_status = convert_xwd_to_png(&xwd_target, path);
    let _ = std::fs::remove_file(&xwd_path);
    matches!(convert_status, Ok(status) if status.success()) && file_has_bytes(path)
}

fn prepare_x11_window_for_capture(window_id: &str) {
    let _ = Command::new("xdotool")
        .args(["windowraise", window_id])
        .status();
    std::thread::sleep(Duration::from_millis(80));
}

fn take_x11_screenshot_with_import(window_id: &str, path: &str) -> bool {
    if import_to_target(window_id, path) && file_has_bytes(path) {
        return true;
    }

    let output_target = format!("png:{path}");
    import_to_target(window_id, &output_target) && file_has_bytes(path)
}

fn import_to_target(window_id: &str, output_target: &str) -> bool {
    matches!(
        Command::new("import")
            .args(["-silent", "-window", window_id, output_target])
            .status(),
        Ok(status) if status.success()
    )
}

fn convert_xwd_to_png(xwd_target: &str, path: &str) -> std::io::Result<std::process::ExitStatus> {
    match Command::new("magick").args([xwd_target, path]).status() {
        Ok(status) => Ok(status),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            Command::new("convert").args([xwd_target, path]).status()
        }
        Err(err) => Err(err),
    }
}

fn file_has_bytes(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0)
}

fn find_visible_process_window_id(title: &str, process_id: &str) -> Option<String> {
    let output = Command::new("xdotool")
        .args(["search", "--onlyvisible", "--name", title])
        .output()
        .expect("xdotool");
    let ids: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect();

    ids.iter()
        .rev()
        .find(|id| window_belongs_to_process(id, process_id) && window_title_matches(id, title))
        .cloned()
        .or_else(|| {
            ids.iter()
                .rev()
                .find(|id| window_belongs_to_process(id, process_id))
                .cloned()
        })
        .or_else(|| {
            ids.iter()
                .rev()
                .find(|id| window_title_matches(id, title))
                .cloned()
        })
}

fn window_belongs_to_process(window_id: &str, process_id: &str) -> bool {
    command_stdout("xdotool", &["getwindowpid", window_id])
        .is_some_and(|pid| pid.trim() == process_id)
}

fn window_title_matches(window_id: &str, title: &str) -> bool {
    command_stdout("xdotool", &["getwindowname", window_id])
        .is_some_and(|name| name.trim() == title)
}

fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        None
    }
}

#[allow(dead_code)]
pub(crate) fn open_text_tab(robot: &cranpose::Robot) {
    for _ in 0..3 {
        if let Some((x, y, w, h)) = wait_for_button_in_semantics(robot, "Shaders", 10) {
            robot
                .click(x + w * 0.5, y + h * 0.5)
                .expect("click Shaders tab");
            std::thread::sleep(Duration::from_millis(250));
            let _ = robot.wait_for_idle();
        }

        let Some((x, y, w, h)) = wait_for_exact_button_in_semantics(robot, "Text", 20) else {
            continue;
        };
        robot
            .click(x + w * 0.5, y + h * 0.5)
            .expect("click Text tab");
        std::thread::sleep(Duration::from_millis(350));
        let _ = robot.wait_for_idle();

        if wait_for_text_in_semantics(robot, "Text Rendering Feature Showcase", 30) {
            return;
        }
    }

    panic!("Text showcase heading not found after tab switch");
}

#[allow(dead_code)]
pub(crate) fn wait_for_text_showcase_heading(robot: &cranpose::Robot) {
    assert!(
        wait_for_text_in_semantics(robot, "Text Rendering Feature Showcase", 30),
        "Text showcase heading not found"
    );
}

pub(crate) fn find_exact_text_in_semantics(
    robot: &cranpose::Robot,
    text: &str,
) -> Option<(f32, f32, f32, f32)> {
    find_in_semantics(robot, |root| find_text_exact(root, text))
}

fn wait_for_button_in_semantics(
    robot: &cranpose::Robot,
    text: &str,
    attempts: usize,
) -> Option<(f32, f32, f32, f32)> {
    for _ in 0..attempts {
        if let Some(bounds) = find_button_in_semantics(robot, text) {
            return Some(bounds);
        }
        std::thread::sleep(Duration::from_millis(100));
        let _ = robot.wait_for_idle();
    }
    None
}

fn wait_for_exact_button_in_semantics(
    robot: &cranpose::Robot,
    text: &str,
    attempts: usize,
) -> Option<(f32, f32, f32, f32)> {
    for _ in 0..attempts {
        if let Some(bounds) = find_button_exact_in_semantics(robot, text) {
            return Some(bounds);
        }
        std::thread::sleep(Duration::from_millis(100));
        let _ = robot.wait_for_idle();
    }
    None
}

fn wait_for_text_in_semantics(robot: &cranpose::Robot, text: &str, attempts: usize) -> bool {
    for _ in 0..attempts {
        if find_text_in_semantics(robot, text).is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
        let _ = robot.wait_for_idle();
    }
    false
}

#[allow(dead_code)]
pub(crate) fn scroll_text_into_view(
    robot: &cranpose::Robot,
    text: &str,
    window_height: u32,
    max_attempts: usize,
) {
    scroll_text_into_view_between(
        robot,
        text,
        100.0,
        window_height as f32 - 100.0,
        max_attempts,
    );
}

pub(crate) fn scroll_text_into_view_between(
    robot: &cranpose::Robot,
    text: &str,
    min_center_y: f32,
    max_center_y: f32,
    max_attempts: usize,
) {
    assert!(
        min_center_y < max_center_y,
        "invalid target center range: {min_center_y}..{max_center_y}"
    );
    let diagnostic = std::env::var_os("CRANPOSE_TEXT_SCROLL_HELPER_DIAGNOSTIC").is_some();
    let mut last_seen_bounds = None;
    for attempt in 0..max_attempts {
        let mut scroll_anchor = (240.0, (min_center_y + max_center_y) * 0.5);
        let mut scroll_delta_y = -80.0;
        if let Some(bounds) = find_exact_text_in_semantics(robot, text) {
            let center_y = bounds.1 + bounds.3 * 0.5;
            last_seen_bounds = Some((bounds, center_y));
            if diagnostic {
                let exact_bounds = collect_exact_text_bounds(robot, text);
                println!(
                    "scroll helper attempt={attempt} seen bounds={bounds:?} center_y={center_y:.2} exact_bounds={exact_bounds:?}"
                );
            }
            if center_y > min_center_y && center_y < max_center_y {
                return;
            }
            scroll_anchor = (
                bounds.0 + bounds.2 * 0.5,
                center_y.clamp(min_center_y + 1.0, max_center_y - 1.0),
            );
            if center_y < min_center_y {
                scroll_delta_y = 80.0;
            }
        } else if diagnostic {
            let visible_story_titles = collect_texts_containing(robot, "Robot HN Story");
            println!(
                "scroll helper attempt={attempt} target missing visible_story_titles={visible_story_titles:?}"
            );
        }
        if diagnostic {
            println!(
                "scroll helper attempt={attempt} anchor=({:.2},{:.2}) delta_y={scroll_delta_y:.2}",
                scroll_anchor.0, scroll_anchor.1
            );
        }
        robot
            .mouse_move(scroll_anchor.0, scroll_anchor.1)
            .expect("move cursor to scroll anchor");
        std::thread::sleep(Duration::from_millis(30));
        robot
            .mouse_scroll(0.0, scroll_delta_y)
            .expect("scroll to find text");
        std::thread::sleep(Duration::from_millis(200));
        let _ = robot.wait_for_idle();
    }
    if let Some((bounds, center_y)) = last_seen_bounds {
        panic!(
            "text '{text}' was seen at bounds {bounds:?} with center_y={center_y:.2}, but never entered target center range {min_center_y:.2}..{max_center_y:.2} after {max_attempts} scroll attempts"
        );
    }
    panic!("text '{text}' not found after {max_attempts} scroll attempts");
}

fn collect_exact_text_bounds(robot: &cranpose::Robot, text: &str) -> Vec<(f32, f32, f32, f32)> {
    let Ok(semantics) = robot.get_semantics() else {
        return Vec::new();
    };
    let mut bounds = Vec::new();
    for root in &semantics {
        collect_exact_text_bounds_from(root, text, &mut bounds);
    }
    bounds
}

fn collect_texts_containing(robot: &cranpose::Robot, needle: &str) -> Vec<String> {
    let Ok(semantics) = robot.get_semantics() else {
        return Vec::new();
    };
    let mut texts = Vec::new();
    for root in &semantics {
        collect_texts_containing_from(root, needle, &mut texts);
    }
    texts.sort();
    texts.dedup();
    texts
}

fn collect_texts_containing_from(
    elem: &cranpose::SemanticElement,
    needle: &str,
    texts: &mut Vec<String>,
) {
    if let Some(text) = elem.text.as_deref() {
        if text.contains(needle) {
            texts.push(text.to_string());
        }
    }
    for child in &elem.children {
        collect_texts_containing_from(child, needle, texts);
    }
}

fn collect_exact_text_bounds_from(
    elem: &cranpose::SemanticElement,
    text: &str,
    bounds: &mut Vec<(f32, f32, f32, f32)>,
) {
    if elem.text.as_deref() == Some(text) {
        bounds.push((
            elem.bounds.x,
            elem.bounds.y,
            elem.bounds.width,
            elem.bounds.height,
        ));
    }
    for child in &elem.children {
        collect_exact_text_bounds_from(child, text, bounds);
    }
}
