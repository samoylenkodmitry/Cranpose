// The benchmark's log marker: `SHOPBENCH home_ready`, written once per
// process once the frame that shows Home's loaded content is on the screen.

use std::sync::atomic::{AtomicBool, Ordering};

use cranpose::{LaunchedEffect, composable};

static WRITTEN: AtomicBool = AtomicBool::new(false);

// The frame that composes this effect also draws the content and presents it.
// Frame callbacks run at the start of the next frame, after that present, so
// the line waits for one.
#[composable]
pub(crate) fn ReportHomeReady() {
    LaunchedEffect((), async move |scope| {
        if WRITTEN.load(Ordering::Relaxed) {
            return;
        }
        diagnostic("SHOPBENCH_DIAG content composed");
        scope.with_frame_nanos(|_| ()).await;
        if !WRITTEN.swap(true, Ordering::Relaxed) {
            write_line("SHOPBENCH home_ready");
        }
    });
}

// The `diag` feature writes the moment the content is composed, to check the
// marker against the system's own record of the frames; the measured build
// writes only the marker, as the benchmark asks.
#[cfg(feature = "diag")]
fn diagnostic(line: &str) {
    write_line(line);
}

#[cfg(not(feature = "diag"))]
fn diagnostic(_line: &str) {}

#[cfg(target_os = "ios")]
fn write_line(line: &str) {
    oslog::OsLog::global().with_level(oslog::Level::Default, line);
}

#[cfg(not(target_os = "ios"))]
fn write_line(line: &str) {
    log::info!("{line}");
}
