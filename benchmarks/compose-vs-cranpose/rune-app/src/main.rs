//! The gauntlet in Rune: the screen `compose-app/.../Gauntlet.kt` draws, in
//! Rune's own idiom. The benchmark's README describes it; `desktop.py` runs
//! it: `PERF_TIER=16 PERF_FONTS=../fonts cargo run --release`.

mod chat;
mod gauntlet;
mod patchwork;
mod wall;

use std::sync::OnceLock;

use patchwork::Patchwork;
use perf_data::*;
use rune::{AppSpec, Color, Family, TextStyle, color::srgb_decode};

/// An sRGB byte color as Rune holds it, in linear light.
pub(crate) fn rgb(value: [u8; 3]) -> Color {
    let linear = |byte: u8| srgb_decode(f32::from(byte) / 255.0);
    Color {
        r: linear(value[0]),
        g: linear(value[1]),
        b: linear(value[2]),
        a: 1.0,
    }
}

/// Roboto at `size` points, lines 1.4 em apart as the other apps set them.
pub(crate) fn roboto(size: f32, bold: bool) -> TextStyle {
    TextStyle {
        line_height: size * 1.4,
        family: Family::Named("Roboto"),
        ..TextStyle::new(size, if bold { 700.0 } else { 400.0 })
    }
}

/// The font files of the screen, read once and kept for the program's life,
/// as `AppSpec` holds fonts: Roboto, and the patchwork's code face.
pub(crate) fn fonts() -> &'static [&'static [u8]] {
    static FONTS: OnceLock<&'static [&'static [u8]]> = OnceLock::new();
    FONTS.get_or_init(|| {
        let files: &[&str] = match perf_data::launch().scenario {
            Scenario::Patchwork => &[
                "Roboto-Regular.ttf",
                "Roboto-Bold.ttf",
                "DejaVuSansMono.ttf",
            ],
            Scenario::Wall | Scenario::Gauntlet => &["Roboto-Regular.ttf", "Roboto-Bold.ttf"],
        };
        let faces = files
            .iter()
            .filter_map(|file| match font_bytes(file) {
                Ok(bytes) => Some(&*Box::leak(bytes.into_boxed_slice())),
                Err(error) => {
                    log::warn!("no {file}: {error}");
                    None
                }
            })
            .collect::<Vec<&'static [u8]>>();
        Box::leak(faces.into_boxed_slice())
    })
}

fn main() {
    perf_data::log_to_stdout();
    let (width, height) = DESKTOP_WINDOW;
    let size = (width as f32, height as f32);
    let title = perf_data::launch().scenario.title();
    match perf_data::launch().scenario {
        Scenario::Wall => rune::run(AppSpec {
            fonts: fonts(),
            families: ("Roboto", "Roboto"),
            poll: Some(wall::poll),
            ..AppSpec::new(title, size, wall::Wall::new, wall::view)
        }),
        Scenario::Gauntlet => rune::run(AppSpec {
            fonts: fonts(),
            families: ("Roboto", "Roboto"),
            setup: Some(|app: &mut gauntlet::App, wake| app.setup(wake)),
            poll: Some(gauntlet::poll),
            ..AppSpec::new(title, size, gauntlet::App::new, gauntlet::view)
        }),
        Scenario::Patchwork => rune::run(AppSpec {
            fonts: fonts(),
            families: ("Roboto", "DejaVu Sans Mono"),
            setup: Some(|patchwork: &mut Patchwork, wake| patchwork.setup(wake)),
            poll: Some(patchwork::poll),
            ..AppSpec::new(title, size, Patchwork::new, patchwork::view)
        }),
    }
}
