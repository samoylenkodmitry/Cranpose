//! Desktop preview: `cargo run --release -- --scenario=ticker`. `desktop.py`
//! runs the gauntlet with `--scenario=gauntlet --tier=N` and `PERF_FONTS`.

fn main() {
    perf_data::log_to_stdout();
    if let Err(error) = perf_compare::run_desktop() {
        eprintln!("Failed to launch: {error}");
        std::process::exit(1);
    }
}
