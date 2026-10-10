//! What the iOS and watchOS hosts share.

use std::{path::PathBuf, sync::OnceLock, time::Duration};

use cranpose_services::PlatformDirectories;
use dispatch2::{DispatchQueue, DispatchTime};
use log::{Level, LevelFilter, Log, Metadata, Record};
use objc2::MainThreadMarker;
use objc2_foundation::{NSBundle, NSHomeDirectory};
use oslog::OsLog;

/// Sends the process's logs to the unified system log, once, under the app's
/// bundle id: `log stream --predicate 'subsystem == "<bundle id>"'` shows them.
/// An app that installed its own logger keeps it.
pub(crate) fn init_logging() {
    static LOGGER: OnceLock<SystemLog> = OnceLock::new();
    let logger = LOGGER.get_or_init(|| {
        let subsystem = NSBundle::mainBundle()
            .bundleIdentifier()
            .map_or_else(|| "dev.cranpose".to_owned(), |id| id.to_string());
        SystemLog(OsLog::new(&subsystem, "app"))
    });
    if log::set_logger(logger).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }
}

struct SystemLog(OsLog);

impl Log for SystemLog {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
            && !(metadata.level() > Level::Warn
                && ["wgpu_core", "wgpu_hal", "naga"]
                    .iter()
                    .any(|noisy| metadata.target().starts_with(noisy)))
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // The system keeps Default and above; it drops Info unless asked.
        let level = match record.level() {
            Level::Error => oslog::Level::Error,
            Level::Warn | Level::Info => oslog::Level::Default,
            Level::Debug | Level::Trace => oslog::Level::Debug,
        };
        self.0
            .with_level(level, &format!("{}: {}", record.target(), record.args()));
    }

    fn flush(&self) {}
}

/// The folders of the application's sandbox.
pub(crate) fn sandbox_directories() -> PlatformDirectories {
    let home = PathBuf::from(NSHomeDirectory().to_string());
    let library = home.join("Library");
    PlatformDirectories {
        data: library.join("Application Support"),
        config: library.join("Preferences"),
        cache: library.join("Caches"),
        documents: Some(home.join("Documents")),
        temporary: std::env::temp_dir(),
        shared: None,
    }
}

/// Runs `work` on the main thread `after` from now: at once when called there
/// with no delay.
pub(crate) fn on_main_after(after: Duration, work: impl FnOnce(MainThreadMarker) + Send + 'static) {
    if after.is_zero()
        && let Some(mtm) = MainThreadMarker::new()
    {
        work(mtm);
        return;
    }
    let run = move || {
        if let Some(mtm) = MainThreadMarker::new() {
            work(mtm);
        }
    };
    match DispatchTime::try_from(after) {
        Ok(when) if !after.is_zero() => {
            let _ = DispatchQueue::main().after(when, run);
        }
        _ => DispatchQueue::main().exec_async(run),
    }
}

/// The start of each buzz of a vibration pattern, from the pattern's start,
/// with the buzz's strength. Apple watches and phones play taps, so a pattern
/// becomes one tap where each buzz starts.
pub(crate) fn buzz_starts(
    pattern: &cranpose_services::HapticPattern,
) -> impl Iterator<Item = (Duration, u8)> + '_ {
    let mut start = Duration::ZERO;
    let mut was_on = false;
    pattern
        .timings_ms()
        .iter()
        .zip(pattern.amplitudes())
        .filter_map(move |(&timing, &amplitude)| {
            let on = amplitude > 0 && timing > 0;
            let buzz = (on && !was_on).then_some((start, amplitude));
            was_on = on;
            start += Duration::from_millis(u64::from(timing));
            buzz
        })
}
