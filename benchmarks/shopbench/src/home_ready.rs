// The benchmark's log marker: `SHOPBENCH home_ready`, written once per
// process when Home first composes its loaded content.

use std::sync::atomic::{AtomicBool, Ordering};

use cranpose::composable;
use cranpose_core::SideEffect;

static WRITTEN: AtomicBool = AtomicBool::new(false);

// The benchmark's apps write the line right after Home's data reaches the
// screen's views, before the frame that shows them is drawn; SwiftUI writes
// it from `onAppear` of the loaded content. A side effect runs at that point:
// once the composition that holds the content is applied, before layout and
// drawing.
#[composable]
pub(crate) fn ReportHomeReady() {
    SideEffect(|| {
        if !WRITTEN.swap(true, Ordering::Relaxed) {
            write_line("SHOPBENCH home_ready");
        }
    });
}

#[cfg(target_os = "ios")]
fn write_line(line: &str) {
    oslog::OsLog::global().with_level(oslog::Level::Default, line);
}

#[cfg(not(target_os = "ios"))]
fn write_line(line: &str) {
    log::info!("{line}");
}
