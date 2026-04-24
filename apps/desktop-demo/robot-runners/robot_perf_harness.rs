//! Robot performance harness for CPU profiling and memory growth validation.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_perf_harness --features robot-app
//! CRANPOSE_PERF_SCENARIO=backdrop_blur cargo run --package desktop-app --example robot_perf_harness --features robot-app
//! ```

use cranpose::fps_stats;
use cranpose::AppLauncher;
use cranpose_foundation::lazy::{
    remember_lazy_list_state, LazyLayoutStats, LazyListScope, LazyListState,
};
use cranpose_render_wgpu::RenderStatsSnapshot;
use cranpose_ui::widgets::{
    Box, BoxSpec, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Row, RowSpec, Text,
};
use cranpose_ui::{
    composable, Color, GraphicsLayer, LinearArrangement, Modifier, RenderEffect, TextStyle,
};
use std::cell::RefCell;
use std::time::{Duration, Instant};

const DEFAULT_DURATION_SECS: u64 = 3;
const DEFAULT_WARMUP_SECS: u64 = 5;
const DEFAULT_SAMPLE_INTERVAL_MS: u64 = 200;
const DEFAULT_MAX_GROWTH_KB: u64 = 512 * 1024;
const DEFAULT_TIMEOUT_SLACK_SECS: u64 = 20;
const WARN_GROWTH_KB: u64 = 100 * 1024;
const ALERT_GROWTH_KB: u64 = 300 * 1024;
const SCROLL_X_DEFAULT: f32 = 450.0;
const SCROLL_X_BACKDROP: f32 = 700.0;
const SCROLL_START_Y: f32 = 590.0;
const SCROLL_END_Y: f32 = 180.0;
const LAZY_STATS_HOOK: &str = "perf.lazy_stats";

thread_local! {
    static PERF_LAZY_LIST_STATE: RefCell<Option<LazyListState>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PerfScenario {
    LazyListScroll,
    TextHeavyScroll,
    BackdropBlur,
    OpaqueScene,
}

impl PerfScenario {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "lazy_list_scroll" => Some(Self::LazyListScroll),
            "text_heavy_scroll" => Some(Self::TextHeavyScroll),
            "backdrop_blur" => Some(Self::BackdropBlur),
            "opaque_scene" => Some(Self::OpaqueScene),
            _ => None,
        }
    }

    fn from_env() -> Self {
        std::env::var("CRANPOSE_PERF_SCENARIO")
            .ok()
            .as_deref()
            .and_then(Self::parse)
            .unwrap_or(Self::LazyListScroll)
    }

    fn name(self) -> &'static str {
        match self {
            Self::LazyListScroll => "lazy_list_scroll",
            Self::TextHeavyScroll => "text_heavy_scroll",
            Self::BackdropBlur => "backdrop_blur",
            Self::OpaqueScene => "opaque_scene",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::LazyListScroll => "Lazy List Scroll",
            Self::TextHeavyScroll => "Text-Heavy Scroll",
            Self::BackdropBlur => "Backdrop Blur Panel",
            Self::OpaqueScene => "Opaque Scene",
        }
    }

    fn item_count(self) -> usize {
        match self {
            Self::LazyListScroll => 280,
            Self::TextHeavyScroll => 180,
            Self::BackdropBlur => 220,
            Self::OpaqueScene => 260,
        }
    }

    fn drag_x(self) -> f32 {
        match self {
            Self::BackdropBlur => SCROLL_X_BACKDROP,
            _ => SCROLL_X_DEFAULT,
        }
    }

    fn overlay_color(self) -> Color {
        match self {
            Self::LazyListScroll => Color(0.36, 0.62, 0.88, 0.9),
            Self::TextHeavyScroll => Color(0.82, 0.58, 0.32, 0.88),
            Self::BackdropBlur => Color(0.72, 0.86, 0.98, 0.24),
            Self::OpaqueScene => Color(0.3, 0.36, 0.44, 1.0),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RenderStatsAccumulator {
    samples: u64,
    submits: u64,
    offscreen_acquires: u64,
    offscreen_news: u64,
    offscreen_total_bytes: u64,
    upload_bytes: u64,
    isolated_layer_renders: u64,
    isolated_layer_pixels: u64,
    layer_cache_hits: u64,
    layer_cache_misses: u64,
    layer_cache_evictions: u64,
    layer_cache_hit_pixels: u64,
    layer_cache_miss_pixels: u64,
    blur_passes: u64,
    composite_passes: u64,
    effect_applies: u64,
    shape_passes: u64,
    image_passes: u64,
    text_passes: u64,
    max_upload_bytes: u64,
    max_isolated_layer_pixels: u64,
}

impl RenderStatsAccumulator {
    fn record(&mut self, stats: RenderStatsSnapshot) {
        self.samples = self.samples.saturating_add(1);
        self.submits = self.submits.saturating_add(stats.submits as u64);
        self.offscreen_acquires = self
            .offscreen_acquires
            .saturating_add(stats.offscreen_acquires as u64);
        self.offscreen_news = self
            .offscreen_news
            .saturating_add(stats.offscreen_news as u64);
        self.offscreen_total_bytes = self
            .offscreen_total_bytes
            .saturating_add(stats.offscreen_total_bytes);
        self.upload_bytes = self.upload_bytes.saturating_add(stats.upload_bytes);
        self.isolated_layer_renders = self
            .isolated_layer_renders
            .saturating_add(stats.isolated_layer_renders as u64);
        self.isolated_layer_pixels = self
            .isolated_layer_pixels
            .saturating_add(stats.isolated_layer_pixels);
        self.layer_cache_hits = self
            .layer_cache_hits
            .saturating_add(stats.layer_cache_hits as u64);
        self.layer_cache_misses = self
            .layer_cache_misses
            .saturating_add(stats.layer_cache_misses as u64);
        self.layer_cache_evictions = self
            .layer_cache_evictions
            .saturating_add(stats.layer_cache_evictions as u64);
        self.layer_cache_hit_pixels = self
            .layer_cache_hit_pixels
            .saturating_add(stats.layer_cache_hit_pixels);
        self.layer_cache_miss_pixels = self
            .layer_cache_miss_pixels
            .saturating_add(stats.layer_cache_miss_pixels);
        self.blur_passes = self.blur_passes.saturating_add(stats.blur_passes as u64);
        self.composite_passes = self
            .composite_passes
            .saturating_add(stats.composite_passes as u64);
        self.effect_applies = self
            .effect_applies
            .saturating_add(stats.effect_applies as u64);
        self.shape_passes = self.shape_passes.saturating_add(stats.shape_passes as u64);
        self.image_passes = self.image_passes.saturating_add(stats.image_passes as u64);
        self.text_passes = self.text_passes.saturating_add(stats.text_passes as u64);
        self.max_upload_bytes = self.max_upload_bytes.max(stats.upload_bytes);
        self.max_isolated_layer_pixels = self
            .max_isolated_layer_pixels
            .max(stats.isolated_layer_pixels);
    }

    fn average_u64(&self, total: u64) -> u64 {
        total.checked_div(self.samples).unwrap_or(0)
    }

    fn cache_hit_rate_pct(self) -> f64 {
        let total = self.layer_cache_hits + self.layer_cache_misses;
        if total == 0 {
            0.0
        } else {
            (self.layer_cache_hits as f64 / total as f64) * 100.0
        }
    }
}

#[composable]
#[allow(non_snake_case)]
fn PerfHarnessApp(scenario: PerfScenario) {
    let list_state = remember_lazy_list_state();
    PERF_LAZY_LIST_STATE.with(|slot| {
        *slot.borrow_mut() = Some(list_state);
    });

    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::new(),
        move || {
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .background(Color(0.06, 0.07, 0.09, 1.0)),
                BoxSpec::new(),
                || {},
            );

            Column(
                Modifier::empty().fill_max_size().padding(14.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
                {
                    move || {
                        Text(
                            format!("Perf Harness: {}", scenario.title()),
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                        Text(
                            format!("Scenario key: {}", scenario.name()),
                            Modifier::empty(),
                            TextStyle::default(),
                        );

                        ScenarioViewport(list_state, scenario);
                    }
                },
            );
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn ScenarioViewport(list_state: LazyListState, scenario: PerfScenario) {
    Box(
        Modifier::empty().fill_max_width().height(610.0),
        BoxSpec::new(),
        {
            move || {
                Box(
                    Modifier::empty()
                        .fill_max_size()
                        .background(Color(0.03, 0.04, 0.06, 1.0))
                        .rounded_corners(18.0),
                    BoxSpec::new(),
                    || {},
                );

                Box(
                    Modifier::empty().fill_max_size().padding(12.0),
                    BoxSpec::new(),
                    {
                        move || {
                            PerfScenarioList(list_state, scenario);
                            if scenario == PerfScenario::BackdropBlur {
                                BackdropOverlayCard();
                            }
                        }
                    },
                );
            }
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn PerfScenarioList(list_state: LazyListState, scenario: PerfScenario) {
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list_state,
        LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move |scope| populate_perf_items(scope, scenario),
    );
}

fn populate_perf_items(scope: &mut impl LazyListScope, scenario: PerfScenario) {
    let item_count = scenario.item_count();
    scope.items(
        item_count,
        Some(|i: usize| i as u64),
        None::<fn(usize) -> u64>,
        move |i| {
            PerfScenarioItem(i, scenario);
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn PerfScenarioItem(index: usize, scenario: PerfScenario) {
    match scenario {
        PerfScenario::LazyListScroll => CacheRow(index, scenario),
        PerfScenario::TextHeavyScroll => TextHeavyRow(index, scenario),
        PerfScenario::BackdropBlur => BackdropRow(index),
        PerfScenario::OpaqueScene => OpaqueRow(index),
    }
}

#[composable]
#[allow(non_snake_case)]
fn CacheRow(index: usize, scenario: PerfScenario) {
    let card = if index.is_multiple_of(2) {
        Color(0.11, 0.13, 0.18, 1.0)
    } else {
        Color(0.09, 0.11, 0.16, 1.0)
    };
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(76.0)
            .background(card)
            .rounded_corners(14.0)
            .padding(14.0),
        BoxSpec::new(),
        move || {
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(12.0)),
                move || {
                    Column(
                        Modifier::empty(),
                        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                        move || {
                            Text(
                                format!("Cache row {}", index),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                            Text(
                                format!("Rigid-motion candidate {}", index % 11),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                        },
                    );
                    PerfBadge(format!("chip {}", index % 9), scenario.overlay_color());
                },
            );
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn TextHeavyRow(index: usize, scenario: PerfScenario) {
    let card = if index.is_multiple_of(2) {
        Color(0.14, 0.11, 0.09, 1.0)
    } else {
        Color(0.16, 0.13, 0.1, 1.0)
    };
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(124.0)
            .background(card)
            .rounded_corners(16.0)
            .padding(14.0),
        BoxSpec::new(),
        move || {
            Column(
                Modifier::empty(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                move || {
                    Text(
                        format!("Paragraph card {}", index),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                    Text(
                        "Renderer counters should show cache reuse while text-heavy cards move rigidly under scroll."
                            .to_string(),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                    Row(
                        Modifier::empty(),
                        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(10.0)),
                        move || {
                            Text(
                                "Glyph shaping stays hot here.".to_string(),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                            PerfBadge(format!("t{}", index % 13), scenario.overlay_color());
                        },
                    );
                },
            );
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn BackdropRow(index: usize) {
    let base = if index.is_multiple_of(3) {
        Color(0.18, 0.24, 0.32, 1.0)
    } else if index % 3 == 1 {
        Color(0.24, 0.18, 0.28, 1.0)
    } else {
        Color(0.16, 0.28, 0.24, 1.0)
    };
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(88.0)
            .background(base)
            .rounded_corners(14.0)
            .padding(14.0),
        BoxSpec::new(),
        move || {
            Column(
                Modifier::empty(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                move || {
                    Text(
                        format!("Backdrop band {}", index),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                    Text(
                        "High-frequency color blocks make blur and backdrop costs obvious."
                            .to_string(),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                },
            );
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn OpaqueRow(index: usize) {
    let primary = if index.is_multiple_of(2) {
        Color(0.18, 0.2, 0.24, 1.0)
    } else {
        Color(0.14, 0.16, 0.2, 1.0)
    };
    let accent_a = Color(0.32, 0.44, 0.58, 1.0);
    let accent_b = Color(0.22, 0.34, 0.46, 1.0);
    let accent_c = Color(0.14, 0.24, 0.34, 1.0);
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(68.0)
            .background(primary)
            .rounded_corners(12.0)
            .padding(12.0),
        BoxSpec::new(),
        move || {
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(10.0)),
                move || {
                    OpaqueBlock(accent_a);
                    OpaqueBlock(accent_b);
                    OpaqueBlock(accent_c);
                },
            );
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn OpaqueBlock(color: Color) {
    Box(
        Modifier::empty()
            .width(120.0)
            .height(40.0)
            .background(color)
            .rounded_corners(8.0),
        BoxSpec::new(),
        || {},
    );
}

#[composable]
#[allow(non_snake_case)]
fn PerfBadge(label: String, color: Color) {
    Box(
        Modifier::empty()
            .graphics_layer_value(GraphicsLayer {
                alpha: 0.86,
                ..Default::default()
            })
            .background(color)
            .rounded_corners(12.0)
            .padding(10.0),
        BoxSpec::new(),
        move || {
            Text(label.clone(), Modifier::empty(), TextStyle::default());
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn BackdropOverlayCard() {
    Box(
        Modifier::empty()
            .offset(86.0, 96.0)
            .width(320.0)
            .height(172.0)
            .backdrop_effect(RenderEffect::blur(14.0))
            .background(Color(0.78, 0.86, 0.96, 0.24))
            .rounded_corners(20.0)
            .padding(18.0),
        BoxSpec::new(),
        move || {
            Column(
                Modifier::empty(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    Text(
                        "Backdrop panel".to_string(),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                    Text(
                        "This scenario should allocate bounded local surfaces, not frame-sized offscreens."
                            .to_string(),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                    PerfBadge("blur".to_string(), Color(0.66, 0.78, 0.96, 0.55));
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

fn timeout_slack_secs_from(value: Option<&str>) -> u64 {
    value
        .and_then(|raw| raw.parse::<u64>().ok())
        .unwrap_or(DEFAULT_TIMEOUT_SLACK_SECS)
}

fn timeout_budget_secs(duration_secs: u64, warmup_secs: u64, timeout_slack_secs: u64) -> u64 {
    duration_secs
        .saturating_add(warmup_secs)
        .saturating_add(timeout_slack_secs)
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

fn fatal(robot: &cranpose::Robot, message: impl std::fmt::Display) -> ! {
    eprintln!("FATAL: {message}");
    let _ = robot.exit();
    std::process::exit(1);
}

fn wait_for_perf_idle(robot: &cranpose::Robot) {
    if robot.wait_for_idle().is_err() {
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn perf_isolate_debug_enabled() -> bool {
    env_bool("CRANPOSE_PERF_LOG_ISOLATES", false)
}

fn print_render_summary(scenario: PerfScenario, stats: RenderStatsAccumulator) {
    println!(
        "PERF_RENDER_SUMMARY scenario={} samples={} avg_submits={} avg_offscreen_acquires={} avg_offscreen_bytes={} avg_upload_bytes={} max_upload_bytes={} avg_isolated_layers={} avg_isolated_pixels={} max_isolated_pixels={} cache_hits={} cache_misses={} cache_hit_rate_pct={:.2} cache_evictions={} avg_blur_passes={} avg_composite_passes={} avg_effect_applies={} avg_shape_passes={} avg_image_passes={} avg_text_passes={}",
        scenario.name(),
        stats.samples,
        stats.average_u64(stats.submits),
        stats.average_u64(stats.offscreen_acquires),
        stats.average_u64(stats.offscreen_total_bytes),
        stats.average_u64(stats.upload_bytes),
        stats.max_upload_bytes,
        stats.average_u64(stats.isolated_layer_renders),
        stats.average_u64(stats.isolated_layer_pixels),
        stats.max_isolated_layer_pixels,
        stats.layer_cache_hits,
        stats.layer_cache_misses,
        stats.cache_hit_rate_pct(),
        stats.layer_cache_evictions,
        stats.average_u64(stats.blur_passes),
        stats.average_u64(stats.composite_passes),
        stats.average_u64(stats.effect_applies),
        stats.average_u64(stats.shape_passes),
        stats.average_u64(stats.image_passes),
        stats.average_u64(stats.text_passes),
    );
}

fn print_memory_summary(
    scenario: PerfScenario,
    baseline_rss_kb: Option<u64>,
    peak_rss_kb: u64,
    sample_count: u64,
) {
    if let Some(baseline) = baseline_rss_kb {
        println!(
            "PERF_MEMORY_SUMMARY scenario={} baseline_rss_kb={} peak_rss_kb={} growth_kb={} samples={}",
            scenario.name(),
            baseline,
            peak_rss_kb,
            peak_rss_kb.saturating_sub(baseline),
            sample_count,
        );
    } else {
        println!(
            "PERF_MEMORY_SUMMARY scenario={} baseline_rss_kb=unavailable peak_rss_kb=unavailable growth_kb=unavailable samples={}",
            scenario.name(),
            sample_count,
        );
    }
}

fn lazy_reuse_rate_pct(stats: &LazyLayoutStats) -> f64 {
    if stats.total_composed == 0 {
        0.0
    } else {
        (stats.reuse_count as f64 / stats.total_composed as f64) * 100.0
    }
}

fn format_lazy_summary(stats: LazyLayoutStats) -> String {
    format!(
        "items_in_use={} items_in_pool={} total_composed={} reuse_count={} reuse_rate_pct={:.2}",
        stats.items_in_use,
        stats.items_in_pool,
        stats.total_composed,
        stats.reuse_count,
        lazy_reuse_rate_pct(&stats),
    )
}

fn lazy_summary_from_app_thread() -> Option<String> {
    PERF_LAZY_LIST_STATE.with(|slot| {
        let state = *slot.borrow();
        state.map(|state| format_lazy_summary(state.stats()))
    })
}

fn print_lazy_summary(robot: &cranpose::Robot, scenario: PerfScenario) {
    let summary = robot
        .invoke_app_hook(LAZY_STATS_HOOK, "")
        .unwrap_or_else(|err| fatal(robot, format!("failed to read lazy stats: {err}")));

    match summary {
        Some(summary) => println!("PERF_LAZY_SUMMARY scenario={} {}", scenario.name(), summary),
        None => println!(
            "PERF_LAZY_SUMMARY scenario={} unavailable=true",
            scenario.name()
        ),
    }
}

fn print_runtime_summary(robot: &cranpose::Robot, scenario: PerfScenario) {
    let stats = robot
        .get_runtime_leak_debug_stats()
        .unwrap_or_else(|err| fatal(robot, format!("failed to read runtime stats: {err}")));

    println!(
        "PERF_RUNTIME_SUMMARY scenario={} groups={} payloads={} nodes={} active_anchors={} detached_anchors={} invalidated_anchors={} free_anchors={} anchor_capacity={} retained_subtrees={} retained_groups={} retained_payloads={} retained_nodes={} retained_scopes={} retained_anchors={} retained_heap_bytes={}",
        scenario.name(),
        stats.slot_stats.group_count,
        stats.slot_stats.payload_count,
        stats.slot_stats.node_count,
        stats.slot_stats.active_anchor_count,
        stats.slot_stats.detached_anchor_count,
        stats.slot_stats.invalidated_anchor_count,
        stats.slot_stats.free_anchor_count,
        stats.slot_stats.anchor_capacity,
        stats.slot_stats.retained_subtree_count,
        stats.slot_stats.retained_group_count,
        stats.slot_stats.retained_payload_count,
        stats.slot_stats.retained_node_count,
        stats.slot_stats.retained_scope_count,
        stats.slot_stats.retained_anchor_count,
        stats.slot_stats.retained_heap_bytes,
    );
}

fn main() {
    env_logger::init();
    let scenario = PerfScenario::from_env();
    let duration_secs = env_u64("CRANPOSE_PERF_DURATION_SECS", DEFAULT_DURATION_SECS);
    let warmup_secs = env_u64("CRANPOSE_PERF_WARMUP_SECS", DEFAULT_WARMUP_SECS);
    let sample_interval_ms = env_u64(
        "CRANPOSE_MEM_SAMPLE_INTERVAL_MS",
        DEFAULT_SAMPLE_INTERVAL_MS,
    );
    let max_growth_kb = env_u64("CRANPOSE_MEM_MAX_GROWTH_KB", DEFAULT_MAX_GROWTH_KB);
    let validate_mem = env_bool("CRANPOSE_MEM_VALIDATE", true);
    let timeout_slack_secs = timeout_slack_secs_from(
        std::env::var("CRANPOSE_PERF_TIMEOUT_SLACK_SECS")
            .ok()
            .as_deref(),
    );

    println!("=== Robot Perf Harness ===");
    println!("Scenario: {} ({})", scenario.name(), scenario.title());
    println!("Duration: {}s (warmup {}s)", duration_secs, warmup_secs);
    println!(
        "Memory validation: {} (max growth {} KB, sample {} ms)",
        validate_mem, max_growth_kb, sample_interval_ms
    );

    AppLauncher::new()
        .with_title(format!("Robot Perf Harness - {}", scenario.title()))
        .with_size(900, 700)
        .with_headless(env_bool("CRANPOSE_HEADLESS", true))
        .with_robot_app_hook(|name, _argument| match name.as_str() {
            LAZY_STATS_HOOK => Ok(lazy_summary_from_app_thread()),
            _ => Err(format!("unknown robot app hook: {name}")),
        })
        .with_test_driver(move |robot| {
            let timeout_secs = timeout_budget_secs(duration_secs, warmup_secs, timeout_slack_secs);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(timeout_secs));
                eprintln!("TIMEOUT: Perf harness exceeded {} seconds", timeout_secs);
                std::process::exit(1);
            });

            std::thread::sleep(Duration::from_millis(500));
            wait_for_perf_idle(&robot);

            let total_duration = Duration::from_secs(duration_secs + warmup_secs);
            let warmup_duration = Duration::from_secs(warmup_secs);
            let sample_interval = Duration::from_millis(sample_interval_ms);
            let mut next_sample = Instant::now() + sample_interval;
            let mut baseline_rss_kb = None;
            let mut peak_rss_kb = 0u64;
            let mut sample_count = 0u64;
            let mut direction_down = true;
            let mut iteration = 0u64;
            let mut render_stats = RenderStatsAccumulator::default();
            let start = Instant::now();

            while start.elapsed() < total_duration {
                let (from_y, to_y) = if direction_down {
                    (SCROLL_START_Y, SCROLL_END_Y)
                } else {
                    (SCROLL_END_Y, SCROLL_START_Y)
                };
                direction_down = !direction_down;

                if let Err(err) = robot.drag(scenario.drag_x(), from_y, scenario.drag_x(), to_y) {
                    fatal(&robot, err);
                }
                wait_for_perf_idle(&robot);

                let elapsed = start.elapsed();
                if baseline_rss_kb.is_none() && elapsed >= warmup_duration {
                    baseline_rss_kb = read_rss_kb();
                    if let Some(rss) = baseline_rss_kb {
                        peak_rss_kb = rss;
                    }
                }

                if elapsed >= warmup_duration {
                    match robot.get_render_stats() {
                        Ok(Some(snapshot)) => render_stats.record(snapshot),
                        Ok(None) => {}
                        Err(err) => fatal(&robot, err),
                    }
                }

                if validate_mem && Instant::now() >= next_sample {
                    if let Some(rss) = read_rss_kb() {
                        if baseline_rss_kb.is_some() {
                            peak_rss_kb = peak_rss_kb.max(rss);
                            sample_count = sample_count.saturating_add(1);
                        }
                    }
                    next_sample += sample_interval;
                }

                iteration = iteration.saturating_add(1);
            }

            if validate_mem {
                if let Some(baseline) = baseline_rss_kb {
                    let growth = peak_rss_kb.saturating_sub(baseline);
                    let growth_mb = growth / 1024;

                    if growth < WARN_GROWTH_KB {
                        println!(
                            "✓ Memory: baseline {} MB | peak {} MB | growth {} MB (healthy)",
                            baseline / 1024,
                            peak_rss_kb / 1024,
                            growth_mb
                        );
                    } else if growth < ALERT_GROWTH_KB {
                        println!(
                            "⚠ Memory: baseline {} MB | peak {} MB | growth {} MB (needs attention)",
                            baseline / 1024,
                            peak_rss_kb / 1024,
                            growth_mb
                        );
                    } else if growth < max_growth_kb {
                        eprintln!(
                            "⚠️  WARNING: Memory growth {} MB is high (baseline {} MB, peak {} MB)",
                            growth_mb,
                            baseline / 1024,
                            peak_rss_kb / 1024
                        );
                    }
                    println!("Samples: {}", sample_count);

                    if growth > max_growth_kb {
                        fatal(
                            &robot,
                            format!(
                                "RSS growth {} MB exceeds limit {} MB",
                                growth_mb,
                                max_growth_kb / 1024
                            ),
                        );
                    }
                } else {
                    println!("RSS unavailable - memory validation skipped");
                }
            }

            print_memory_summary(scenario, baseline_rss_kb, peak_rss_kb, sample_count);
            print_render_summary(scenario, render_stats);
            print_lazy_summary(&robot, scenario);
            print_runtime_summary(&robot, scenario);
            if perf_isolate_debug_enabled() {
                match robot.get_render_stats() {
                    Ok(Some(snapshot)) => {
                        for (index, layer) in snapshot.top_isolated_layers().enumerate() {
                            println!(
                                "PERF_ISOLATED scenario={} rank={} node={:?} rect=({:.1},{:.1},{:.1},{:.1}) target={}x{} reasons={}",
                                scenario.name(),
                                index,
                                layer.node_id,
                                layer.logical_rect.x,
                                layer.logical_rect.y,
                                layer.logical_rect.width,
                                layer.logical_rect.height,
                                layer.width,
                                layer.height,
                                layer.reasons.display(),
                            );
                        }
                    }
                    Ok(None) => {}
                    Err(err) => fatal(&robot, err),
                }
            }

            let stats = fps_stats();
            println!(
                "PERF_FPS_SUMMARY scenario={} fps={:.1} avg_frame_ms={:.2} total_frames={} recompositions={} recompositions_per_second={}",
                scenario.name(),
                stats.fps,
                stats.avg_ms,
                stats.frame_count,
                stats.recompositions,
                stats.recomps_per_second,
            );
            println!(
                "PERF_SCENARIO_COMPLETE scenario={} iterations={}",
                scenario.name(),
                iteration
            );

            robot.exit().ok();
        })
        .run(move || {
            PerfHarnessApp(scenario);
        });
}

#[cfg(test)]
mod tests {
    use super::{
        format_lazy_summary, lazy_reuse_rate_pct, timeout_budget_secs, timeout_slack_secs_from,
        LazyLayoutStats, PerfScenario, RenderStatsAccumulator, RenderStatsSnapshot,
        DEFAULT_TIMEOUT_SLACK_SECS,
    };

    #[test]
    fn timeout_slack_uses_default_for_missing_or_invalid_values() {
        assert_eq!(timeout_slack_secs_from(None), DEFAULT_TIMEOUT_SLACK_SECS);
        assert_eq!(
            timeout_slack_secs_from(Some("invalid")),
            DEFAULT_TIMEOUT_SLACK_SECS
        );
    }

    #[test]
    fn timeout_slack_parses_valid_override() {
        assert_eq!(timeout_slack_secs_from(Some("180")), 180);
    }

    #[test]
    fn timeout_budget_adds_duration_warmup_and_slack() {
        assert_eq!(timeout_budget_secs(2, 3, 180), 185);
    }

    #[test]
    fn perf_scenario_parses_known_keys() {
        assert_eq!(
            PerfScenario::parse("lazy_list_scroll"),
            Some(PerfScenario::LazyListScroll)
        );
        assert_eq!(
            PerfScenario::parse("text_heavy_scroll"),
            Some(PerfScenario::TextHeavyScroll)
        );
        assert_eq!(
            PerfScenario::parse("backdrop_blur"),
            Some(PerfScenario::BackdropBlur)
        );
        assert_eq!(
            PerfScenario::parse("opaque_scene"),
            Some(PerfScenario::OpaqueScene)
        );
        assert_eq!(PerfScenario::parse("nope"), None);
    }

    #[test]
    fn render_stats_accumulator_tracks_uploads_and_cache_rate() {
        let mut stats = RenderStatsAccumulator::default();
        stats.record(RenderStatsSnapshot {
            submits: 2,
            offscreen_acquires: 3,
            offscreen_news: 1,
            offscreen_total_bytes: 4096,
            upload_bytes: 512,
            isolated_layer_renders: 1,
            isolated_layer_pixels: 1024,
            layer_cache_hits: 4,
            layer_cache_misses: 1,
            layer_cache_evictions: 0,
            layer_cache_hit_pixels: 300,
            layer_cache_miss_pixels: 120,
            blur_passes: 2,
            composite_passes: 3,
            effect_applies: 1,
            shape_passes: 5,
            image_passes: 0,
            text_passes: 2,
            offscreen_pool_size: 0,
            offscreen_pool_bytes: 0,
            text_pool_size: 0,
            layer_cache_size: 0,
            layer_cache_bytes: 0,
            image_cache_size: 0,
            text_cache_size: 0,
            ..RenderStatsSnapshot::default()
        });
        stats.record(RenderStatsSnapshot {
            upload_bytes: 256,
            layer_cache_hits: 1,
            layer_cache_misses: 1,
            ..RenderStatsSnapshot::default()
        });

        assert_eq!(stats.samples, 2);
        assert_eq!(stats.upload_bytes, 768);
        assert_eq!(stats.max_upload_bytes, 512);
        assert_eq!(stats.average_u64(stats.upload_bytes), 384);
        assert!((stats.cache_hit_rate_pct() - 71.428).abs() < 0.01);
    }

    #[test]
    fn lazy_reuse_rate_is_zero_without_compositions() {
        assert_eq!(lazy_reuse_rate_pct(&LazyLayoutStats::default()), 0.0);
    }

    #[test]
    fn lazy_summary_reports_reuse_rate() {
        let summary = format_lazy_summary(LazyLayoutStats {
            items_in_use: 6,
            items_in_pool: 3,
            total_composed: 20,
            reuse_count: 15,
        });

        assert_eq!(
            summary,
            "items_in_use=6 items_in_pool=3 total_composed=20 reuse_count=15 reuse_rate_pct=75.00",
        );
    }
}
