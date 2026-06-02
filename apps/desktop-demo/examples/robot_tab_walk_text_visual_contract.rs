use cranpose::AppLauncher;
use cranpose_testing::{find_in_semantics, find_text_exact};
use desktop_app::app::{self, DemoTab, TEST_ACTIVE_TAB_STATE};
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const WINDOW_WIDTH: u32 = 1083;
const WINDOW_HEIGHT: u32 = 813;
const WINDOW_TITLE: &str = "Robot Tab Walk Text Visual Contract";
const HOLD_ENV: &str = "CRANPOSE_TAB_WALK_VISUAL_HOLD_MS";
const CAPTURE_ONLY_ENV: &str = "CRANPOSE_TAB_WALK_VISUAL_CAPTURE_ONLY";

#[derive(Clone, Copy, Debug)]
struct InkMetrics {
    pixels: usize,
    min_x: u32,
    max_x: u32,
    min_y: u32,
    max_y: u32,
}

impl InkMetrics {
    fn width(self) -> u32 {
        self.max_x.saturating_sub(self.min_x) + 1
    }

    fn height(self) -> u32 {
        self.max_y.saturating_sub(self.min_y) + 1
    }
}

fn main() {
    let _ = env_logger::try_init();
    println!("=== Robot Tab Walk Text Visual Contract ===");

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(false)
        .with_robot_app_hook(set_tab_hook)
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_millis(700));
            let _ = robot.wait_for_idle();

            walk_tabs_to_text(&robot);
            wait_for_exact_text(&robot, "Text Rendering Feature Showcase", 30);
            wait_for_exact_text(&robot, "Serif Bold Italic", 30);
            wait_for_exact_text(&robot, "Decorated shadow text", 30);

            if !capture_only() {
                let window_id = find_window_id(WINDOW_TITLE);
                let output_dir = diagnostic_dir("cranpose_tab_walk_text_visual_contract");
                let full_path = output_dir.join("text_tab_after_walk.png");
                let image = capture_x11_window(&window_id, &full_path);
                println!("Captured final Text tab: {}", full_path.display());

                let serif_bounds = find_exact_text_bounds(&robot, "Serif Bold Italic");
                assert_text_ink(
                    &image,
                    serif_bounds,
                    is_serif_yellow_pixel,
                    "Serif Bold Italic",
                    0.42,
                    7,
                    28,
                );

                let decorated_bounds = find_exact_text_bounds(&robot, "Decorated shadow text");
                assert_text_ink(
                    &image,
                    decorated_bounds,
                    is_light_text_pixel,
                    "Decorated shadow text",
                    0.55,
                    7,
                    55,
                );
            }

            let hold_ms = std::env::var(HOLD_ENV)
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(400);
            if hold_ms > 0 {
                std::thread::sleep(Duration::from_millis(hold_ms));
            }

            println!("PASS: Text tab presented pixels survived tab walk");
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn capture_only() -> bool {
    std::env::var_os(CAPTURE_ONLY_ENV).is_some_and(|value| value != "0")
}

fn walk_tabs_to_text(robot: &cranpose::Robot) {
    for tab in ["mineswapper2", "images", "lazy-list", "text"] {
        set_active_tab(robot, tab);
        std::thread::sleep(Duration::from_millis(180));
        let _ = robot.wait_for_idle();
    }
}

fn set_active_tab(robot: &cranpose::Robot, tab: &str) {
    robot
        .invoke_app_hook("set-tab", tab)
        .unwrap_or_else(|err| panic!("failed to select tab '{tab}': {err}"));
}

fn set_tab_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name != "set-tab" {
        return Err(format!("unsupported robot app hook {name}({argument})"));
    }
    let tab = match argument.as_str() {
        "mineswapper2" => DemoTab::Mineswapper2,
        "images" => DemoTab::Images,
        "lazy-list" => DemoTab::LazyList,
        "text" => DemoTab::Text,
        _ => return Err(format!("unknown demo tab '{argument}'")),
    };
    let state = TEST_ACTIVE_TAB_STATE
        .with(|cell| cell.borrow().as_ref().copied())
        .unwrap_or_else(|| panic!("active tab state was not installed before selecting {tab:?}"));
    state.set(tab);
    Ok(None)
}

fn wait_for_exact_text(robot: &cranpose::Robot, text: &str, attempts: usize) {
    for _ in 0..attempts {
        if find_exact_text_bounds(robot, text).2 > 0.0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
        let _ = robot.wait_for_idle();
    }
    panic!("text '{text}' not found in semantics");
}

fn find_exact_text_bounds(robot: &cranpose::Robot, text: &str) -> (f32, f32, f32, f32) {
    find_in_semantics(robot, |root| find_text_exact(root, text))
        .unwrap_or_else(|| panic!("text '{text}' not found in semantics"))
}

fn find_window_id(title: &str) -> String {
    let process_id = std::process::id().to_string();
    for _ in 0..30 {
        let ids = command_stdout("xdotool", &["search", "--onlyvisible", "--name", title])
            .unwrap_or_default();
        for id in ids.lines().rev().map(str::trim).filter(|id| !id.is_empty()) {
            if window_title_matches(id, title) && window_belongs_to_process(id, &process_id) {
                return id.to_owned();
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    panic!("window '{title}' not found for current process");
}

fn window_title_matches(window_id: &str, title: &str) -> bool {
    command_stdout("xdotool", &["getwindowname", window_id])
        .is_some_and(|name| name.trim() == title)
}

fn window_belongs_to_process(window_id: &str, process_id: &str) -> bool {
    command_stdout("xdotool", &["getwindowpid", window_id])
        .is_some_and(|pid| pid.trim() == process_id)
}

fn command_stdout(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        None
    }
}

fn diagnostic_dir(name: &str) -> PathBuf {
    let root = std::env::var_os("CRANPOSE_ROBOT_OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(".cranpose-tmp")
        });
    let path = root.join(name);
    std::fs::create_dir_all(&path)
        .unwrap_or_else(|err| panic!("failed to create {}: {err}", path.display()));
    path
}

fn capture_x11_window(window_id: &str, path: &Path) -> RgbaImage {
    let target = format!("png:{}", path.display());
    let status = Command::new("import")
        .args(["-silent", "-window", window_id, &target])
        .status()
        .unwrap_or_else(|err| panic!("failed to run import for {}: {err}", path.display()));
    assert!(status.success(), "import failed for {}", path.display());
    image::open(path)
        .unwrap_or_else(|err| panic!("failed to open {}: {err}", path.display()))
        .to_rgba8()
}

fn assert_text_ink(
    image: &RgbaImage,
    bounds: (f32, f32, f32, f32),
    predicate: fn([u8; 4]) -> bool,
    label: &str,
    min_width_ratio: f32,
    min_height_px: u32,
    min_pixels: usize,
) {
    let crop = crop_pixels(image, bounds, 8.0);
    let metrics = ink_metrics(image, crop, predicate)
        .unwrap_or_else(|| panic!("{label} has no matching presented ink pixels"));
    let scale_x = image.width() as f32 / WINDOW_WIDTH as f32;
    let min_width = (bounds.2 * scale_x * min_width_ratio).ceil().max(1.0) as u32;

    println!(
        "{label} ink metrics: pixels={} width={} height={} required_width>={}",
        metrics.pixels,
        metrics.width(),
        metrics.height(),
        min_width
    );

    assert!(
        metrics.pixels >= min_pixels
            && metrics.width() >= min_width
            && metrics.height() >= min_height_px,
        "{label} presented ink is too sparse or too small after tab walk: {metrics:?}, bounds={bounds:?}, image={}x{}",
        image.width(),
        image.height()
    );
}

fn crop_pixels(image: &RgbaImage, bounds: (f32, f32, f32, f32), pad: f32) -> (u32, u32, u32, u32) {
    let scale_x = image.width() as f32 / WINDOW_WIDTH as f32;
    let scale_y = image.height() as f32 / WINDOW_HEIGHT as f32;
    let left = ((bounds.0 - pad) * scale_x).floor().max(0.0) as u32;
    let top = ((bounds.1 - pad) * scale_y).floor().max(0.0) as u32;
    let right = ((bounds.0 + bounds.2 + pad) * scale_x)
        .ceil()
        .min(image.width() as f32) as u32;
    let bottom = ((bounds.1 + bounds.3 + pad) * scale_y)
        .ceil()
        .min(image.height() as f32) as u32;
    (left, top, right.max(left + 1), bottom.max(top + 1))
}

fn ink_metrics(
    image: &RgbaImage,
    crop: (u32, u32, u32, u32),
    predicate: fn([u8; 4]) -> bool,
) -> Option<InkMetrics> {
    let (left, top, right, bottom) = crop;
    let mut metrics: Option<InkMetrics> = None;
    for y in top..bottom {
        for x in left..right {
            let pixel = image.get_pixel(x, y).0;
            if !predicate(pixel) {
                continue;
            }
            metrics = Some(match metrics {
                Some(current) => InkMetrics {
                    pixels: current.pixels + 1,
                    min_x: current.min_x.min(x),
                    max_x: current.max_x.max(x),
                    min_y: current.min_y.min(y),
                    max_y: current.max_y.max(y),
                },
                None => InkMetrics {
                    pixels: 1,
                    min_x: x,
                    max_x: x,
                    min_y: y,
                    max_y: y,
                },
            });
        }
    }
    metrics
}

fn is_serif_yellow_pixel(pixel: [u8; 4]) -> bool {
    let [red, green, blue, alpha] = pixel;
    alpha >= 180 && red >= 150 && green >= 120 && blue >= 70 && blue <= 225 && red >= green
}

fn is_light_text_pixel(pixel: [u8; 4]) -> bool {
    let [red, green, blue, alpha] = pixel;
    alpha >= 180 && red >= 145 && green >= 145 && blue >= 145
}
