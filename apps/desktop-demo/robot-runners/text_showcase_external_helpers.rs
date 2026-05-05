use cranpose_testing::{
    find_button_exact_in_semantics, find_button_in_semantics, find_text_in_semantics,
};
use std::process::Command;
use std::time::Duration;

pub(crate) fn find_window_id(title: &str) -> String {
    for _ in 0..20 {
        let output = Command::new("xdotool")
            .args(["search", "--name", title])
            .output()
            .expect("xdotool");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let id = stdout
            .trim()
            .lines()
            .last()
            .unwrap_or("")
            .trim()
            .to_string();
        if !id.is_empty() {
            return id;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    panic!("window '{title}' not found via xdotool");
}

pub(crate) fn take_x11_screenshot(window_id: &str, path: &str) {
    let status = Command::new("import")
        .args(["-window", window_id, path])
        .status()
        .expect("import command");
    assert!(status.success(), "import failed for {path}");
}

#[allow(dead_code)]
pub(crate) fn open_text_tab(robot: &cranpose::Robot) {
    let (x, y, w, h) = find_button_in_semantics(robot, "Shaders").expect("Shaders tab");
    robot
        .click(x + w * 0.5, y + h * 0.5)
        .expect("click Shaders tab");
    std::thread::sleep(Duration::from_millis(250));
    let _ = robot.wait_for_idle();

    let (x, y, w, h) = find_button_exact_in_semantics(robot, "Text").expect("Text tab");
    robot
        .click(x + w * 0.5, y + h * 0.5)
        .expect("click Text tab");
    std::thread::sleep(Duration::from_millis(350));
    let _ = robot.wait_for_idle();
    assert!(
        find_text_in_semantics(robot, "Text Rendering Feature Showcase").is_some(),
        "Text showcase heading not found after tab switch"
    );
}

pub(crate) fn scroll_text_into_view(
    robot: &cranpose::Robot,
    text: &str,
    window_height: u32,
    max_attempts: usize,
) {
    for _ in 0..max_attempts {
        if let Some(bounds) = find_text_in_semantics(robot, text) {
            let center_y = bounds.1 + bounds.3 * 0.5;
            if center_y > 100.0 && center_y < (window_height as f32 - 100.0) {
                return;
            }
        }
        robot
            .mouse_move(600.0, 450.0)
            .expect("move cursor to center");
        std::thread::sleep(Duration::from_millis(30));
        robot
            .mouse_scroll(0.0, -80.0)
            .expect("scroll down to find text");
        std::thread::sleep(Duration::from_millis(200));
        let _ = robot.wait_for_idle();
    }
    panic!("text '{text}' not found after {max_attempts} scroll attempts");
}
