//! The gauntlet in a desktop window, as `desktop.py` runs it:
//! `PERF_TIER=12 PERF_FONTS=../fonts cargo run --release`.

fn main() {
    perf_data::log_to_stdout();
    let (width, height) = perf_data::DESKTOP_WINDOW;
    let viewport = eframe::egui::ViewportBuilder::default()
        .with_title("Gauntlet")
        .with_inner_size([width as f32, height as f32])
        .with_resizable(false);
    perf_egui::run(
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        perf_data::Launch::from_env(),
    );
}
