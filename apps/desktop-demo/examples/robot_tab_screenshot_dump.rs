//! Per-tab screenshot dump used to visually diff renderer backends.
//!
//! Drives the desktop demo app through every [`DemoTab`], lets each tab
//! settle, and writes one PNG per tab into the directory named by the
//! `ROBOT_SHOT_DIR` environment variable. This binary links against
//! whichever renderer feature the crate was compiled with (`renderer-wgpu`
//! the two output directories (e.g. with ImageMagick `compare`) catches
//! renderer-specific pixel regressions tab by tab.
//!
//! ```bash
//! ROBOT_SHOT_DIR=/tmp/shots CRANPOSE_HEADLESS=0 \
//!     cargo run --profile robot -p desktop-app --features robot-app \
//!     --example robot_tab_screenshot_dump
//! ```
//!
//! A handful of tabs pull live network content or run continuous
//! animations; their pixels are not reproducible run to run and are listed
//! in [`NONDETERMINISTIC_TABS`] so callers can treat a nonzero diff on those
//! tabs as expected rather than as a renderer gap.

use cranpose::AppLauncher;
use desktop_app::app::{self, DemoTab, DEMO_TABS, TEST_ACTIVE_TAB_STATE};
use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::time::Duration;

const WINDOW_WIDTH: u32 = 1200;
const WINDOW_HEIGHT: u32 = 800;
const WINDOW_TITLE: &str = "Robot Tab Screenshot Dump";
const SHOT_DIR_ENV: &str = "ROBOT_SHOT_DIR";
const SETTLE_MS_ENV: &str = "ROBOT_SHOT_SETTLE_MS";

/// Tabs whose captured pixels are expected to vary between runs: network
/// fetches race against a fixed settle timer, and continuously animating
/// tabs are sampled mid-animation. A nonzero diff on these tabs is not on
/// its own evidence of a renderer regression; inspect the diff image before
/// drawing conclusions.
const NONDETERMINISTIC_TABS: [DemoTab; 7] = [
    DemoTab::HackerNews,
    DemoTab::WebFetch,
    DemoTab::Xkcd,
    DemoTab::Animations,
    DemoTab::InteractiveAnim,
    DemoTab::ShaderRect,
    // Scrolls forever: `WearState::advance` runs the list to the end, turns
    // around and runs it back, every frame, with no state it settles into. A
    // capture taken after a fixed settle is therefore taken wherever the scroll
    // happened to be, and two runs of this dump will never agree on it. Left
    // out of the list, that showed up as a large permanent pixel delta on a tab
    // nothing was wrong with -- the exact reading this list exists to prevent.
    DemoTab::Wear,
];

fn main() {
    let _ = env_logger::try_init();
    println!("=== Robot Tab Screenshot Dump ===");

    let shot_dir = shot_dir();
    std::fs::create_dir_all(&shot_dir)
        .unwrap_or_else(|err| panic!("failed to create {}: {err}", shot_dir.display()));

    AppLauncher::new()
        .with_title(WINDOW_TITLE)
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(headless())
        .with_robot_app_hook(set_tab_hook)
        .with_test_driver(move |robot| {
            std::thread::sleep(Duration::from_millis(700));

            for tab in DEMO_TABS {
                dump_tab(&robot, tab, &shot_dir);
            }

            println!(
                "PASS: dumped {} tab screenshots to {}",
                DEMO_TABS.len(),
                shot_dir.display()
            );
            robot.exit().expect("exit");
        })
        .run(app::combined_app);
}

fn dump_tab(robot: &cranpose::Robot, tab: DemoTab, shot_dir: &Path) {
    let slug = tab_slug(tab);
    robot
        .invoke_app_hook("set-tab", slug)
        .unwrap_or_else(|err| panic!("failed to select tab '{slug}': {err}"));

    // The dump includes tabs whose contract is to keep updating forever. A
    // bounded frame pump gives every tab the same opportunity to apply the
    // selection and build its first scene without treating animation as an
    // idle condition.
    robot
        .pump_frames(3)
        .unwrap_or_else(|err| panic!("failed to settle tab '{slug}': {err}"));

    if NONDETERMINISTIC_TABS.contains(&tab) {
        // Give network fetches (or a few more animation frames) a chance to
        // produce representative, if not bit-reproducible, content.
        std::thread::sleep(Duration::from_millis(settle_ms()));
        robot
            .pump_frames(3)
            .unwrap_or_else(|err| panic!("failed to sample tab '{slug}': {err}"));
    }

    let screenshot = robot
        .screenshot()
        .unwrap_or_else(|err| panic!("screenshot failed for tab '{slug}': {err}"));
    let image = RgbaImage::from_raw(screenshot.width, screenshot.height, screenshot.pixels)
        .unwrap_or_else(|| panic!("screenshot buffer for tab '{slug}' had an unexpected size"));
    let path = shot_dir.join(format!("{slug}.png"));
    image
        .save(&path)
        .unwrap_or_else(|err| panic!("failed to write {}: {err}", path.display()));
    println!("captured {slug} -> {}", path.display());
}

/// Single source of truth for the kebab-case slug used both as the
/// `set-tab` app-hook argument and as the screenshot file stem, so the
/// forward (tab -> slug) and reverse (slug -> tab) lookups can never drift
/// apart.
fn tab_slug(tab: DemoTab) -> &'static str {
    match tab {
        DemoTab::Counter => "counter",
        DemoTab::CompositionLocal => "composition-local",
        DemoTab::Async => "async",
        DemoTab::Animations => "animations",
        DemoTab::InteractiveAnim => "interactive-anim",
        DemoTab::WebFetch => "web-fetch",
        DemoTab::TextInput => "text-input",
        DemoTab::Layout => "layout",
        DemoTab::ModifierShowcase => "modifier-showcase",
        DemoTab::LazyList => "lazy-list",
        DemoTab::Mineswapper2 => "mineswapper2",
        DemoTab::HackerNews => "hacker-news",
        DemoTab::Images => "images",
        DemoTab::Text => "text",
        DemoTab::Winamp => "winamp",
        DemoTab::Xkcd => "xkcd",
        DemoTab::Shaders => "shaders",
        DemoTab::ShaderRect => "shader-rect",
        DemoTab::MarkdownViewer => "markdown-viewer",
        DemoTab::Liquid => "liquid-ui",
        DemoTab::FilePicker => "file-picker",
        DemoTab::Rotary => "rotary",
        DemoTab::Wear => "wear-watch",
    }
}

fn set_tab_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name != "set-tab" {
        return Err(format!("unsupported robot app hook {name}({argument})"));
    }
    let tab = DEMO_TABS
        .into_iter()
        .find(|tab| tab_slug(*tab) == argument)
        .ok_or_else(|| format!("unknown demo tab '{argument}'"))?;
    let state = TEST_ACTIVE_TAB_STATE
        .with(|cell| cell.borrow().as_ref().copied())
        .ok_or_else(|| "active tab state was not installed before selecting a tab".to_string())?;
    state.set(tab);
    Ok(None)
}

fn shot_dir() -> PathBuf {
    // Default to a temp directory so the example runs unconfigured (CI robot
    // shards); set ROBOT_SHOT_DIR to keep the screenshots somewhere useful.
    std::env::var_os(SHOT_DIR_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("cranpose-robot-shots"))
}

fn headless() -> bool {
    env_bool("CRANPOSE_HEADLESS", true)
}

fn settle_ms() -> u64 {
    std::env::var(SETTLE_MS_ENV)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(250)
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(default)
}
