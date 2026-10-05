//! The gauntlet in a desktop window, as `desktop.py` runs it:
//! `PERF_TIER=12 PERF_FONTS=../fonts cargo run --release`.

fn main() {
    perf_data::log_to_stdout();
    if let Err(error) = perf_slint::run_desktop(perf_data::Launch::from_env()) {
        log::error!("Slint stopped: {error}");
    }
}
