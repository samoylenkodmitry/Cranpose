//! The gauntlet in a desktop window, as `desktop.py` runs it:
//! `PERF_TIER=16 PERF_FONTS=../fonts cargo run --release`.

fn main() {
    perf_data::log_to_stdout();
    perf_dioxus::run_desktop();
}
