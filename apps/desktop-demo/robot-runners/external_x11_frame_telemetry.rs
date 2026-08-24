use std::{
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

use log::{LevelFilter, Log, Metadata, Record};

static TELEMETRY_RECORDS: OnceLock<Arc<Mutex<Vec<FrameTelemetryRecord>>>> = OnceLock::new();
static LOGGER: TelemetryLogger = TelemetryLogger;

#[derive(Clone, Debug)]
pub(crate) struct FrameTelemetryRecord {
    logged_at: Instant,
    total_ms: f64,
    update_ms: f64,
    acquire_ms: f64,
    render_ms: f64,
    present_ms: f64,
}

#[derive(Debug)]
pub(crate) struct FrameTelemetrySummary {
    pub frames: usize,
    pub fps: f64,
    pub p95_total_ms: f64,
    pub max_total_ms: f64,
    pub p95_update_ms: f64,
    pub p95_acquire_ms: f64,
    pub p95_render_ms: f64,
    pub p95_present_ms: f64,
    pub stalls_50ms: usize,
}

struct TelemetryLogger;

impl Log for TelemetryLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let message = record.args().to_string();
        if should_forward_renderer_telemetry(&message) {
            eprintln!("{message}");
        }
        if !message.contains("[desktop-frame-telemetry:primary]") {
            return;
        }

        let Some(total_ms) = metric(&message, "total_ms") else {
            return;
        };
        let Some(update_ms) = metric(&message, "update_ms") else {
            return;
        };
        let Some(acquire_ms) = metric(&message, "acquire_ms") else {
            return;
        };
        let Some(render_ms) = metric(&message, "render_ms") else {
            return;
        };
        let Some(present_ms) = metric(&message, "present_ms") else {
            return;
        };

        if let Some(records) = TELEMETRY_RECORDS.get() {
            records
                .lock()
                .expect("telemetry records poisoned")
                .push(FrameTelemetryRecord {
                    logged_at: Instant::now(),
                    total_ms,
                    update_ms,
                    acquire_ms,
                    render_ms,
                    present_ms,
                });
        }
    }

    fn flush(&self) {}
}

fn should_forward_renderer_telemetry(message: &str) -> bool {
    message.contains("[wgpu-render-stage:") || message.contains("[frame-stage-telemetry]")
}

pub(crate) fn install_primary_frame_telemetry_logger() -> Arc<Mutex<Vec<FrameTelemetryRecord>>> {
    std::env::set_var("CRANPOSE_DESKTOP_FRAME_TELEMETRY_MS", "0");
    let records = Arc::new(Mutex::new(Vec::new()));
    let _ = TELEMETRY_RECORDS.set(Arc::clone(&records));
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(LevelFilter::Warn);
    records
}

pub(crate) fn clear_records(records: &Arc<Mutex<Vec<FrameTelemetryRecord>>>) {
    records.lock().expect("telemetry records poisoned").clear();
}

pub(crate) fn summarize_records(
    records: &Arc<Mutex<Vec<FrameTelemetryRecord>>>,
    active_start: Instant,
    active_end: Instant,
) -> FrameTelemetrySummary {
    let records = records.lock().expect("telemetry records poisoned");
    let samples = records
        .iter()
        .filter(|record| record.logged_at >= active_start && record.logged_at <= active_end)
        .cloned()
        .collect::<Vec<_>>();
    if samples.is_empty() {
        return FrameTelemetrySummary {
            frames: 0,
            fps: 0.0,
            p95_total_ms: 0.0,
            max_total_ms: 0.0,
            p95_update_ms: 0.0,
            p95_acquire_ms: 0.0,
            p95_render_ms: 0.0,
            p95_present_ms: 0.0,
            stalls_50ms: 0,
        };
    }

    let elapsed = samples
        .last()
        .and_then(|last| {
            samples
                .first()
                .map(|first| last.logged_at.duration_since(first.logged_at))
        })
        .unwrap_or_default()
        .as_secs_f64();
    let fps = if samples.len() >= 2 && elapsed > f64::EPSILON {
        (samples.len() - 1) as f64 / elapsed
    } else {
        0.0
    };
    let total = samples
        .iter()
        .map(|record| record.total_ms)
        .collect::<Vec<_>>();
    let update = samples
        .iter()
        .map(|record| record.update_ms)
        .collect::<Vec<_>>();
    let acquire = samples
        .iter()
        .map(|record| record.acquire_ms)
        .collect::<Vec<_>>();
    let render = samples
        .iter()
        .map(|record| record.render_ms)
        .collect::<Vec<_>>();
    let present = samples
        .iter()
        .map(|record| record.present_ms)
        .collect::<Vec<_>>();
    let max_total_ms = total.iter().copied().fold(0.0, f64::max);
    let stalls_50ms = total.iter().filter(|value| **value >= 50.0).count();

    FrameTelemetrySummary {
        frames: samples.len(),
        fps,
        p95_total_ms: percentile(total, 0.95),
        max_total_ms,
        p95_update_ms: percentile(update, 0.95),
        p95_acquire_ms: percentile(acquire, 0.95),
        p95_render_ms: percentile(render, 0.95),
        p95_present_ms: percentile(present, 0.95),
        stalls_50ms,
    }
}

fn metric(message: &str, name: &str) -> Option<f64> {
    let prefix = format!("{name}=");
    message.split_whitespace().find_map(|part| {
        part.strip_prefix(&prefix)
            .and_then(|value| value.trim_end_matches(',').parse::<f64>().ok())
    })
}

fn percentile(mut values: Vec<f64>, percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() - 1) as f64 * percentile.clamp(0.0, 1.0)).ceil() as usize;
    values[index.min(values.len() - 1)]
}
