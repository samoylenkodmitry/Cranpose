//! The gauntlet and the still wall in Tauri: the page `web-app` draws, in a
//! Tauri window on the system's WKWebView, the engine Safari runs. `desktop.py` serves the page,
//! as it does for Chrome, and passes its address as `--page=URL`. The page
//! asks the app what `desktop.py` asked of it (`launch`) and writes its
//! `PERF` lines through it (`log`), on standard output where `desktop.py`
//! reads them.

use serde::Serialize;
use tauri::WebviewUrl;

/// What `desktop.py` asked of the page, as the page reads it.
#[derive(Serialize)]
struct Launch {
    tier: usize,
    freeze: u32,
    scenario: &'static str,
}

/// What `desktop.py` asked of the page. `shown` is the height the page has,
/// in points: macOS counts the title bar in the size the window was given,
/// so the window first grows until the page is as tall as every app's
/// content.
#[tauri::command]
fn launch(window: tauri::WebviewWindow, shown: f64) -> Launch {
    let (width, height) = perf_data::DESKTOP_WINDOW;
    let missing = f64::from(height) - shown;
    if missing >= 1.0 {
        let grown = tauri::LogicalSize::new(f64::from(width), f64::from(height) + missing);
        if let Err(error) = window.set_size(grown) {
            eprintln!("the window keeps its height: {error}");
        }
    }
    let launch = perf_data::Launch::from_env();
    Launch {
        tier: launch.tier,
        freeze: launch.freeze,
        scenario: launch.scenario.name(),
    }
}

#[tauri::command]
fn log(message: String) {
    println!("{message}");
}

fn main() {
    let Some(page) =
        std::env::args().find_map(|argument| argument.strip_prefix("--page=").map(str::to_owned))
    else {
        eprintln!("usage: perf-compare-tauri --page=URL");
        std::process::exit(2);
    };
    let (width, height) = perf_data::DESKTOP_WINDOW;
    let title = perf_data::Launch::from_env().scenario.title();
    let result = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![launch, log])
        .setup(move |app| {
            let url = WebviewUrl::External(page.parse()?);
            tauri::WebviewWindowBuilder::new(app, "gauntlet", url)
                .title(title)
                .inner_size(f64::from(width), f64::from(height))
                .resizable(false)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("Tauri stopped: {error}");
        std::process::exit(1);
    }
}
