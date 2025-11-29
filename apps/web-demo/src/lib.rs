use compose_app::AppLauncher;
use compose_ui::{composable, Column, ColumnSpec, Modifier, Size, Spacer, Text};
use wasm_bindgen::prelude::*;

// Embedded font data
const ROBOTO_REGULAR: &[u8] = include_bytes!("../../../assets/Roboto-Regular.ttf");

static DEMO_FONTS: &[&[u8]] = &[ROBOTO_REGULAR];

#[wasm_bindgen(start)]
pub fn main() {
    // Set up logging
    wasm_logger::init(wasm_logger::Config::default());
    log::info!("Web demo starting...");
}

#[wasm_bindgen]
pub async fn run_app() -> Result<(), JsValue> {
    log::info!("Initializing Compose app...");

    AppLauncher::new()
        .with_title("Compose Web Demo")
        .with_size(800, 600)
        .with_fonts(&DEMO_FONTS)
        .run_web("compose-canvas", demo_app)
        .await
}

#[composable]
fn demo_app() {
    Column(
        Modifier::empty().padding(20.0),
        ColumnSpec::default(),
        || {
            Text(
                "Hello from RS-Compose!",
                Modifier::empty().padding(4.0),
            );

            Spacer(Size { width: 0.0, height: 20.0 });

            Text(
                "This is running in your browser using WebGPU!",
                Modifier::empty().padding(4.0),
            );

            Spacer(Size { width: 0.0, height: 30.0 });

            Text(
                "Features:",
                Modifier::empty().padding(4.0),
            );

            Spacer(Size { width: 0.0, height: 10.0 });

            Text(
                "✓ Declarative UI framework",
                Modifier::empty().padding(2.0),
            );

            Spacer(Size { width: 0.0, height: 5.0 });

            Text(
                "✓ Rust + WASM + WebGPU",
                Modifier::empty().padding(2.0),
            );

            Spacer(Size { width: 0.0, height: 5.0 });

            Text(
                "✓ Cross-platform (Desktop, Android, Web)",
                Modifier::empty().padding(2.0),
            );
        },
    );
}
