//! Robot performance harness for CPU profiling and memory growth validation.
//!
//! Run with:
//! ```bash
//! # Default (short) run
//! cargo run --package desktop-app --example robot_perf_harness --features robot-app
//!
//! # Longer run for profiling
//! CRANPOSE_PERF_DURATION_SECS=15 cargo run --package desktop-app --example robot_perf_harness --features robot-app
//! ```

use cranpose::AppLauncher;
use cranpose_core::useState;
use cranpose_foundation::lazy::{remember_lazy_list_state, LazyListScope};
use cranpose_testing::find_button_in_semantics;
use cranpose_ui::widgets::{
    Box, BoxSpec, Button, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Row, RowSpec, Text,
};
use cranpose_ui::{composable, Color, LinearArrangement, Modifier};
use std::time::{Duration, Instant};

const DEFAULT_DURATION_SECS: u64 = 3;
const DEFAULT_WARMUP_SECS: u64 = 1;
const DEFAULT_SAMPLE_INTERVAL_MS: u64 = 200;
const DEFAULT_MAX_GROWTH_KB: u64 = 32 * 1024;

#[composable]
#[allow(non_snake_case)]
fn PerfHarnessApp() {
    let toggle = useState(|| false);
    let counter = useState(|| 0u64);
    let list_state = remember_lazy_list_state();

    Column(
        Modifier::empty()
            .fill_max_size()
            .padding(12.0)
            .background(Color(0.08, 0.08, 0.1, 1.0)),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text("Perf Harness".to_string(), Modifier::empty());
            Row(
                Modifier::empty(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    Button(
                        Modifier::empty().background(Color(0.2, 0.4, 0.7, 1.0)),
                        move || {
                            toggle.set(!toggle.get());
                            counter.set(counter.get().saturating_add(1));
                        },
                        || {
                            Text("Toggle".to_string(), Modifier::empty());
                        },
                    );
                    Text(
                        format!("Counter: {}", counter.get()),
                        Modifier::empty().padding(4.0),
                    );
                },
            );

            let toggle_label = if toggle.get() { "ON" } else { "OFF" };
            Text(
                format!("Toggle State: {}", toggle_label),
                Modifier::empty().padding(2.0),
            );

            LazyColumn(
                Modifier::empty()
                    .fill_max_width()
                    .height(420.0)
                    .background(Color(0.05, 0.05, 0.08, 1.0)),
                list_state,
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                |scope| {
                    scope.items(
                        300,
                        Some(|i: usize| i as u64),
                        None::<fn(usize) -> u64>,
                        move |i| {
                            let bg = if i % 2 == 0 {
                                Color(0.12, 0.14, 0.2, 1.0)
                            } else {
                                Color(0.1, 0.12, 0.18, 1.0)
                            };
                            Box(
                                Modifier::empty()
                                    .fill_max_width()
                                    .height(36.0)
                                    .padding(6.0)
                                    .background(bg),
                                BoxSpec::new(),
                                move || {
                                    Text(format!("Item {}", i), Modifier::empty());
                                },
                            );
                        },
                    );
                },
            );
        },
    );
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn env_bool(key: &str, default: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| match value.to_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default,
        })
        .unwrap_or(default)
}

#[cfg(target_os = "linux")]
fn read_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let mut parts = rest.split_whitespace();
            if let Some(value) = parts.next() {
                return value.parse::<u64>().ok();
            }
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn read_rss_kb() -> Option<u64> {
    None
}

fn main() {
    env_logger::init();
    println!("=== Robot Perf Harness ===");

    let duration_secs = env_u64("CRANPOSE_PERF_DURATION_SECS", DEFAULT_DURATION_SECS);
    let warmup_secs = env_u64("CRANPOSE_PERF_WARMUP_SECS", DEFAULT_WARMUP_SECS);
    let sample_interval_ms = env_u64(
        "CRANPOSE_MEM_SAMPLE_INTERVAL_MS",
        DEFAULT_SAMPLE_INTERVAL_MS,
    );
    let max_growth_kb = env_u64("CRANPOSE_MEM_MAX_GROWTH_KB", DEFAULT_MAX_GROWTH_KB);
    let validate_mem = env_bool("CRANPOSE_MEM_VALIDATE", true);

    println!("Duration: {}s (warmup {}s)", duration_secs, warmup_secs);
    println!(
        "Memory validation: {} (max growth {} KB, sample {} ms)",
        validate_mem, max_growth_kb, sample_interval_ms
    );

    AppLauncher::new()
        .with_title("Robot Perf Harness")
        .with_size(900, 700)
        .with_headless(true)
        .with_test_driver(move |robot| {
            let timeout_secs = duration_secs + warmup_secs + 20;
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(timeout_secs));
                eprintln!("TIMEOUT: Perf harness exceeded {} seconds", timeout_secs);
                std::process::exit(1);
            });

            std::thread::sleep(Duration::from_millis(400));
            let _ = robot.wait_for_idle();

            let toggle_center = find_button_in_semantics(&robot, "Toggle")
                .map(|(x, y, w, h)| (x + w / 2.0, y + h / 2.0))
                .unwrap_or_else(|| {
                    eprintln!("FATAL: Toggle button not found");
                    let _ = robot.exit();
                    std::process::exit(1);
                });

            let total_duration = Duration::from_secs(duration_secs + warmup_secs);
            let warmup_duration = Duration::from_secs(warmup_secs);
            let sample_interval = Duration::from_millis(sample_interval_ms);
            let mut next_sample = Instant::now() + sample_interval;
            let mut baseline_rss_kb = None;
            let mut peak_rss_kb = 0u64;
            let mut sample_count = 0u64;
            let mut direction_down = true;

            let start = Instant::now();
            while start.elapsed() < total_duration {
                let (from_y, to_y) = if direction_down {
                    (560.0, 220.0)
                } else {
                    (220.0, 560.0)
                };
                direction_down = !direction_down;

                if let Err(err) = robot.drag(450.0, from_y, 450.0, to_y) {
                    eprintln!("FATAL: Drag failed: {}", err);
                    let _ = robot.exit();
                    std::process::exit(1);
                }

                if let Err(err) = robot.click(toggle_center.0, toggle_center.1) {
                    eprintln!("FATAL: Toggle click failed: {}", err);
                    let _ = robot.exit();
                    std::process::exit(1);
                }

                let _ = robot.wait_for_idle();

                let elapsed = start.elapsed();
                if baseline_rss_kb.is_none() && elapsed >= warmup_duration {
                    baseline_rss_kb = read_rss_kb();
                    if let Some(rss) = baseline_rss_kb {
                        peak_rss_kb = rss;
                    }
                }

                if validate_mem && Instant::now() >= next_sample {
                    if let Some(rss) = read_rss_kb() {
                        if baseline_rss_kb.is_some() {
                            peak_rss_kb = peak_rss_kb.max(rss);
                            sample_count += 1;
                        }
                    }
                    next_sample += sample_interval;
                }
            }

            if validate_mem {
                if let Some(baseline) = baseline_rss_kb {
                    let growth = peak_rss_kb.saturating_sub(baseline);
                    println!(
                        "RSS baseline: {} KB | peak: {} KB | growth: {} KB | samples: {}",
                        baseline, peak_rss_kb, growth, sample_count
                    );
                    if growth > max_growth_kb {
                        eprintln!(
                            "FATAL: RSS growth {} KB exceeds limit {} KB",
                            growth, max_growth_kb
                        );
                        let _ = robot.exit();
                        std::process::exit(1);
                    }
                } else {
                    println!("RSS unavailable - memory validation skipped");
                }
            }

            robot.exit().ok();
        })
        .run(PerfHarnessApp);
}
