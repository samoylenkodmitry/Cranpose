//! Desktop preview: `cargo run --release -- --scenario=ticker`.

fn main() {
    if let Err(error) = perf_compare::create_app().try_run(perf_compare::PerfCompareApp) {
        eprintln!("Failed to launch: {error}");
        std::process::exit(1);
    }
}
