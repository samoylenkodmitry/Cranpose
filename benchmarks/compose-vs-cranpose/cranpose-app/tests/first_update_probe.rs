use cranpose_app_shell::AppShell;
use cranpose_render_pixels::PixelsRenderer;
use perf_compare::{WorkspaceFrame, WorkspaceMode};
use std::time::Instant;

struct StderrLog;
impl log::Log for StderrLog {
    fn enabled(&self, _: &log::Metadata) -> bool { true }
    fn log(&self, record: &log::Record) {
        let msg = format!("{}", record.args());
        if msg.contains("recompose-scope") || msg.contains("stage-telemetry") { eprintln!("{msg}"); }
    }
    fn flush(&self) {}
}
static LOGGER: StderrLog = StderrLog;

#[test]
fn first_update_probe() {
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);
    for (w, h, d) in [(1280u32, 820u32, 1.0f32), (1080, 2143, 3.0)] {
        let mut shell = AppShell::new_with_size_and_density(
            PixelsRenderer::new(),
            cranpose_core::location_key(file!(), line!(), column!()),
            move || WorkspaceFrame(WorkspaceMode::Quotes, false),
            (w, h),
            (w as f32 / d, h as f32 / d),
            d,
        );
        shell.set_semantics_enabled(true);
        eprintln!("== {w}x{h}@{d}: recompose after new = {}", shell.__should_recompose());
        for i in 0..3 {
            let t = Instant::now();
            shell.update();
            eprintln!("update {i}: {:?} recompose after = {}", t.elapsed(), shell.__should_recompose());
        }
    }
}
