//! Crude reproduction probe, NOT a pass/fail contract.
//!
//! Every prior attempt to reproduce the reported "glass chrome renders in
//! jumps while scrolling" bug went through one of two fresh-offscreen-texture
//! paths: `capture_frame_with_scale` (every robot screenshot) or a wgpu-crate
//! integration test. Both paths agree perfectly at every 1px scroll step.
//! Neither path ever touches `redraw_native_window`'s real, reused swapchain
//! view — the only code path a human actually sees.
//!
//! This probe drives the real windowed present path (`with_headless(false)`)
//! on the literal production `Receipts` (GlassFeed) tab, and reads back the
//! literal texture handed to `present()` on every frame via
//! `CRANPOSE_DEBUG_SWAPCHAIN_DUMP_DIR` (see `desktop.rs::
//! dump_swapchain_texture_if_requested`) — no OS-level screen capture, no
//! window automation. Scrolling is driven through the app's own in-process
//! robot channel, the same mechanism every other robot test in this suite
//! uses; only the *readback* is new.
//!
//! It writes one PPM per presented frame and a manifest recording which
//! frame index followed which scroll step, then leaves the files on disk for
//! manual visual inspection — this investigation was burned twice already by
//! trusting a pixel-diff number over an image, so this probe does not
//! compute a diff or a verdict at all.

use std::time::Duration;

use cranpose::AppLauncher;
use desktop_app::app::{self, TEST_ACTIVE_TAB_STATE};

const WINDOW_WIDTH: u32 = 900;
const WINDOW_HEIGHT: u32 = 800;
/// Deep enough that the sampled chrome sits over real card content, not the
/// LazyColumn's still-empty top content padding (the source of this
/// session's first false positive).
const SETTLE_SCROLL: f32 = -400.0;
const STEP_COUNT: usize = 24;

fn main() {
    let _ = env_logger::try_init();

    let dump_dir = std::env::var("CRANPOSE_DEBUG_SWAPCHAIN_DUMP_DIR").unwrap_or_else(|_| {
        let dir =
            std::env::temp_dir().join(format!("cranpose-glass-swapchain-{}", std::process::id()));
        std::env::set_var("CRANPOSE_DEBUG_SWAPCHAIN_DUMP_DIR", &dir);
        dir.to_string_lossy().into_owned()
    });
    println!("swapchain dump dir: {dump_dir}");
    let _ = std::fs::create_dir_all(&dump_dir);

    let manifest_path = std::path::Path::new(&dump_dir).join("manifest.txt");
    let manifest =
        std::sync::Mutex::new(std::fs::File::create(&manifest_path).expect("create manifest"));
    let dump_dir_for_driver = dump_dir.clone();

    AppLauncher::new()
        .with_title("Glass Swapchain Readback Probe")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() == Ok("1"))
        .with_robot_app_hook(set_tab_hook)
        .with_test_driver(move |robot| {
            use std::io::Write;

            std::thread::sleep(Duration::from_millis(700));
            let _ = robot.wait_for_idle();
            robot
                .invoke_app_hook("set-tab", "receipts")
                .expect("select receipts/GlassFeed tab");
            std::thread::sleep(Duration::from_millis(500));
            let _ = robot.wait_for_idle();

            let _ = robot.mouse_scroll_and_wait_for_frame(0.0, SETTLE_SCROLL);
            std::thread::sleep(Duration::from_millis(200));
            let _ = robot.wait_for_idle();

            {
                let mut file = manifest.lock().unwrap();
                let _ = writeln!(
                    file,
                    "after-settle dir_entries={}",
                    count_ppm(&dump_dir_for_driver)
                );
            }

            for step in 0..STEP_COUNT {
                let before = count_ppm(&dump_dir_for_driver);
                let result = robot.mouse_scroll_and_wait_for_frame(0.0, -1.0);
                let after = count_ppm(&dump_dir_for_driver);
                let mut file = manifest.lock().unwrap();
                let _ = writeln!(
                    file,
                    "step={step} result={result:?} frames_before={before} frames_after={after}"
                );
            }

            let _ = robot.exit();
        })
        .try_run(app::combined_app)
        .expect("launch glass swapchain readback probe");

    println!("done. inspect PPMs and manifest under: {dump_dir}");
}

fn count_ppm(dir: &str) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "ppm"))
                .count()
        })
        .unwrap_or(0)
}

fn set_tab_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if name != "set-tab" {
        return Err(format!("unsupported robot app hook {name}({argument})"));
    }
    if argument != "receipts" {
        return Err(format!("unknown demo tab '{argument}'"));
    }
    let state = TEST_ACTIVE_TAB_STATE
        .with(|cell| cell.borrow().as_ref().copied())
        .ok_or_else(|| "active tab state was not installed".to_string())?;
    state.set(app::DemoTab::GlassFeed);
    Ok(None)
}
