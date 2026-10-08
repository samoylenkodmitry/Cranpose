//! The gauntlet in a browser, on WebGL2: `index.html` loads the module and
//! calls [`run_gauntlet`]. The query string is the launch: `?tier=12&freeze=120`
//! reads as `--tier=12 --freeze=120` does on a desktop, and the scenario is
//! the gauntlet unless `?scenario=` says another.

use std::rc::Rc;

use cranpose::{AppLauncher, launch_args_from_command_line, set_platform_launch_args};
use wasm_bindgen::prelude::*;

use crate::PerfCompareApp;

/// The Roboto faces every app draws in, which a page cannot read from files.
const ROBOTO: &[&[u8]] = &[
    include_bytes!("../../fonts/Roboto-Regular.ttf"),
    include_bytes!("../../fonts/Roboto-Medium.ttf"),
    include_bytes!("../../fonts/Roboto-Bold.ttf"),
];

#[wasm_bindgen(start)]
pub fn start() {
    wasm_logger::init(wasm_logger::Config::new(log::Level::Info));
    console_error_panic_hook::set_once();
}

/// The query string as the command line it stands for.
fn launch_tokens(query: &str) -> Vec<String> {
    let mut tokens = vec!["--scenario=gauntlet".to_owned()];
    tokens.extend(
        query
            .trim_start_matches('?')
            .split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| format!("--{pair}")),
    );
    tokens
}

/// Draws the gauntlet in the canvas `canvas_id`.
#[wasm_bindgen]
pub async fn run_gauntlet(canvas_id: &str) -> Result<(), JsValue> {
    let query = web_sys::window()
        .ok_or_else(|| JsValue::from_str("no window"))?
        .location()
        .search()?;
    set_platform_launch_args(Rc::new(launch_args_from_command_line(
        launch_tokens(&query),
        false,
    )));
    AppLauncher::new()
        .with_title("Gauntlet")
        .with_log_tag("PerfCompare")
        .with_fonts(ROBOTO)
        .run_web(canvas_id, PerfCompareApp)
        .await
}
