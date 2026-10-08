//! The gauntlet in a desktop window, as `desktop.py` runs it:
//! `PERF_TIER=12 PERF_FONTS=../fonts cargo run --release`.

use iced::Size;

fn main() -> iced::Result {
    perf_data::log_to_stdout();
    let (width, height) = perf_data::DESKTOP_WINDOW;
    perf_iced::run(
        perf_data::Launch::from_env(),
        Size::new(width as f32, height as f32),
    )
}
