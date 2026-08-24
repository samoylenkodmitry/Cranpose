//! Robot performance harness for CPU profiling and memory growth validation.
//!
//! Run with:
//! ```bash
//! cargo run --package desktop-app --example robot_perf_harness --features robot-app
//! CRANPOSE_PERF_SCENARIO=backdrop_blur cargo run --package desktop-app --example robot_perf_harness --features robot-app
//! ```

mod markdown_fixture_client;
mod perf_robot_stats;

use std::{
    cell::RefCell,
    sync::Arc,
    time::{Duration, Instant},
};

use cranpose::{AppLauncher, LazyItems};
use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, RepeatMode, StartOffset,
};
use cranpose_core::CompositionLocalProvider;
use cranpose_foundation::lazy::{
    rememberLazyListState, LazyLayoutStats, LazyListScope, LazyListState,
};
use cranpose_services::{local_http_client, HttpClientRef};
use cranpose_testing::{find_button_exact_in_semantics, find_text};
use cranpose_ui::{
    composable,
    widgets::{Box, BoxSpec, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Row, RowSpec, Text},
    Color, GraphicsLayer, LinearArrangement, Modifier, RenderEffect, TextStyle,
};
use desktop_app::app;
use markdown_fixture_client::MarkdownFixtureClient;
use perf_robot_stats::{print_render_summary, RenderStatsAccumulator};

const DEFAULT_DURATION_SECS: u64 = 3;
const DEFAULT_WARMUP_SECS: u64 = 5;
const DEFAULT_SAMPLE_INTERVAL_MS: u64 = 200;
const DEFAULT_MAX_GROWTH_KB: u64 = 512 * 1024;
const DEFAULT_TIMEOUT_SLACK_SECS: u64 = 20;
const DEFAULT_MIN_FPS: f32 = 0.0;
const DEFAULT_MAX_P95_FRAME_MS: f32 = 0.0;
const DEFAULT_WAIT_IDLE_AFTER_DRAG: bool = false;
const WARN_GROWTH_KB: u64 = 100 * 1024;
const ALERT_GROWTH_KB: u64 = 300 * 1024;
const SCROLL_X_DEFAULT: f32 = 450.0;
const SCROLL_X_BACKDROP: f32 = 700.0;
const SCROLL_WHEEL_DELTA_Y: f32 = 420.0;
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
    MarkdownScroll,
    MarkdownViewerScroll,
    MarkdownDefaultViewerScroll,
    BackdropBlur,
    OpaqueScene,
    GlassLazyScroll,
}

impl PerfScenario {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "lazy_list_scroll" => Some(Self::LazyListScroll),
            "text_heavy_scroll" => Some(Self::TextHeavyScroll),
            "markdown_scroll" => Some(Self::MarkdownScroll),
            "markdown_viewer_scroll" => Some(Self::MarkdownViewerScroll),
            "markdown_default_viewer_scroll" => Some(Self::MarkdownDefaultViewerScroll),
            "backdrop_blur" => Some(Self::BackdropBlur),
            "opaque_scene" => Some(Self::OpaqueScene),
            "glass_lazy_scroll" => Some(Self::GlassLazyScroll),
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
            Self::MarkdownScroll => "markdown_scroll",
            Self::MarkdownViewerScroll => "markdown_viewer_scroll",
            Self::MarkdownDefaultViewerScroll => "markdown_default_viewer_scroll",
            Self::BackdropBlur => "backdrop_blur",
            Self::OpaqueScene => "opaque_scene",
            Self::GlassLazyScroll => "glass_lazy_scroll",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::LazyListScroll => "Lazy List Scroll",
            Self::TextHeavyScroll => "Text-Heavy Scroll",
            Self::MarkdownScroll => "Markdown Scroll",
            Self::MarkdownViewerScroll => "Fetched Markdown Viewer Scroll",
            Self::MarkdownDefaultViewerScroll => "Default Markdown Viewer Scroll",
            Self::BackdropBlur => "Backdrop Blur Panel",
            Self::OpaqueScene => "Opaque Scene",
            Self::GlassLazyScroll => "Glass Lazy Scroll",
        }
    }

    fn item_count(self) -> usize {
        match self {
            Self::LazyListScroll => 280,
            Self::TextHeavyScroll => 180,
            Self::MarkdownScroll => 0,
            Self::MarkdownViewerScroll => 0,
            Self::MarkdownDefaultViewerScroll => 0,
            Self::BackdropBlur => 220,
            Self::OpaqueScene => 260,
            Self::GlassLazyScroll => 400,
        }
    }

    fn drag_x(self) -> f32 {
        match self {
            Self::BackdropBlur | Self::GlassLazyScroll => SCROLL_X_BACKDROP,
            Self::MarkdownScroll => SCROLL_X_DEFAULT,
            _ => SCROLL_X_DEFAULT,
        }
    }

    fn overlay_color(self) -> Color {
        match self {
            Self::LazyListScroll => Color(0.36, 0.62, 0.88, 0.9),
            Self::TextHeavyScroll => Color(0.82, 0.58, 0.32, 0.88),
            Self::MarkdownScroll => Color(0.55, 0.68, 1.0, 0.88),
            Self::MarkdownViewerScroll => Color(0.50, 0.78, 0.92, 0.88),
            Self::MarkdownDefaultViewerScroll => Color(0.42, 0.80, 0.72, 0.88),
            Self::BackdropBlur => Color(0.72, 0.86, 0.98, 0.24),
            Self::OpaqueScene => Color(0.3, 0.36, 0.44, 1.0),
            Self::GlassLazyScroll => Color(0.72, 0.84, 0.98, 0.9),
        }
    }

    fn input_method(self) -> PerfInputMethod {
        match self {
            Self::MarkdownScroll
            | Self::MarkdownViewerScroll
            | Self::MarkdownDefaultViewerScroll => PerfInputMethod::Wheel,
            _ => PerfInputMethod::Drag,
        }
    }

    fn requires_fetch(self) -> bool {
        matches!(
            self,
            Self::MarkdownViewerScroll | Self::MarkdownDefaultViewerScroll
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PerfInputMethod {
    Drag,
    Wheel,
}

#[composable]
#[allow(non_snake_case)]
fn PerfHarnessApp(scenario: PerfScenario) {
    if scenario == PerfScenario::MarkdownViewerScroll {
        PERF_LAZY_LIST_STATE.with(|slot| {
            *slot.borrow_mut() = None;
        });
        MarkdownViewerPerfApp();
        return;
    }
    if scenario == PerfScenario::MarkdownDefaultViewerScroll {
        PERF_LAZY_LIST_STATE.with(|slot| {
            *slot.borrow_mut() = None;
        });
        MarkdownViewerDefaultFixturePerfApp();
        return;
    }

    let list_state = rememberLazyListState();
    PERF_LAZY_LIST_STATE.with(|slot| {
        *slot.borrow_mut() = Some(list_state);
    });

    if scenario == PerfScenario::MarkdownScroll {
        app::MarkdownScrollStressRobotAppWithState(list_state);
        return;
    }

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
                            if scenario == PerfScenario::GlassLazyScroll {
                                AnimatedGlassOverlay();
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
        LazyItems::new(item_count).key(|i: usize| i as u64),
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
        PerfScenario::MarkdownScroll => TextHeavyRow(index, scenario),
        PerfScenario::MarkdownViewerScroll => TextHeavyRow(index, scenario),
        PerfScenario::MarkdownDefaultViewerScroll => TextHeavyRow(index, scenario),
        PerfScenario::BackdropBlur => BackdropRow(index),
        PerfScenario::OpaqueScene => OpaqueRow(index),
        PerfScenario::GlassLazyScroll => GlassRow(index),
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
fn MarkdownViewerPerfApp() {
    let client = cranpose_core::remember(|| {
        Arc::new(MarkdownFixtureClient::from_body(
            app::markdown_scroll_stress_fixture(),
        )) as HttpClientRef
    })
    .with(|client| client.clone());
    let local = local_http_client();
    CompositionLocalProvider(vec![local.provides(client)], || {
        app::MarkdownViewerRobotApp();
    });
}

#[composable]
#[allow(non_snake_case)]
fn MarkdownViewerDefaultFixturePerfApp() {
    let client = cranpose_core::remember(|| {
        Arc::new(MarkdownFixtureClient::default_leetcode_daily()) as HttpClientRef
    })
    .with(|client| client.clone());
    let local = local_http_client();
    CompositionLocalProvider(vec![local.provides(client)], || {
        app::MarkdownViewerRobotApp();
    });
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

#[composable]
#[allow(non_snake_case)]
fn GlassRow(index: usize) {
    let tint = match index % 4 {
        0 => Color(0.20, 0.26, 0.36, 0.55),
        1 => Color(0.28, 0.20, 0.34, 0.55),
        2 => Color(0.18, 0.32, 0.30, 0.55),
        _ => Color(0.32, 0.26, 0.20, 0.55),
    };
    // Every row blurs what is behind it. That is the difference from
    // `backdrop_blur`, where one overlay blurs once per frame: here the cost
    // scales with the number of rows the viewport holds, which is what a real
    // glass list does.
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(96.0)
            .backdrop_effect(RenderEffect::blur(12.0))
            .background(tint)
            .rounded_corners(18.0)
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
                                format!("Glass row {}", index),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                            Text(
                                "Blurred backdrop, tinted fill, rounded clip.".to_string(),
                                Modifier::empty(),
                                TextStyle::default(),
                            );
                            PerfBadge(
                                format!("layer {}", index % 4),
                                Color(0.70, 0.82, 0.98, 0.55),
                            );
                        },
                    );
                },
            );
        },
    );
}

/// A glass card that never stops moving, so the scenario measures animation
/// alongside blur rather than a still frame that happens to contain both.
#[composable]
#[allow(non_snake_case)]
fn AnimatedGlassOverlay() {
    let transition = rememberInfiniteTransition("glass_overlay");
    let drift = transition.animateFloat(
        0.0,
        1.0,
        infiniteRepeatable(
            AnimationSpec::linear(2_400),
            RepeatMode::Reverse,
            StartOffset::default(),
        ),
        "glass_drift",
    );
    let pulse = transition.animateFloat(
        0.0,
        1.0,
        infiniteRepeatable(
            AnimationSpec::linear(1_700),
            RepeatMode::Reverse,
            StartOffset::default(),
        ),
        "glass_pulse",
    );

    let drift_value = drift.get();
    let pulse_value = pulse.get();

    Box(
        Modifier::empty()
            .offset(48.0 + drift_value * 180.0, 72.0 + drift_value * 96.0)
            .width(300.0)
            .height(164.0)
            .backdrop_effect(RenderEffect::blur(10.0 + pulse_value * 14.0))
            .background(Color(0.80, 0.88, 0.98, 0.20 + pulse_value * 0.12))
            .rounded_corners(22.0)
            .padding(18.0),
        BoxSpec::new(),
        move || {
            Column(
                Modifier::empty(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    Text(
                        "Animated glass".to_string(),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                    Text(
                        "Drifting offset and pulsing blur radius, every frame.".to_string(),
                        Modifier::empty(),
                        TextStyle::default(),
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

fn env_u32(key: &str, default: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default)
}

fn fps_budget_from(value: Option<&str>, default: f32) -> f32 {
    value
        .and_then(|raw| raw.parse::<f32>().ok())
        .filter(|fps| fps.is_finite() && *fps > 0.0)
        .unwrap_or(default)
}

fn frame_ms_budget_from(value: Option<&str>, default: f32) -> f32 {
    value
        .and_then(|raw| raw.parse::<f32>().ok())
        .filter(|ms| ms.is_finite() && *ms > 0.0)
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

fn fatal(_robot: &cranpose::Robot, message: impl std::fmt::Display) -> ! {
    eprintln!("FATAL: {message}");
    std::process::exit(1);
}

fn wait_for_perf_idle(robot: &cranpose::Robot) {
    if robot.wait_for_idle().is_err() {
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn center(bounds: (f32, f32, f32, f32)) -> (f32, f32) {
    (bounds.0 + bounds.2 * 0.5, bounds.1 + bounds.3 * 0.5)
}

fn wait_for_perf_text_bounds(
    robot: &cranpose::Robot,
    text: &str,
    timeout_ms: u64,
) -> Option<(f32, f32, f32, f32)> {
    let attempts = (timeout_ms / 100).max(1);
    for _ in 0..attempts {
        if let Ok(semantics) = robot.get_semantics() {
            if let Some(bounds) = semantics.iter().find_map(|elem| find_text(elem, text)) {
                return Some(bounds);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
        wait_for_perf_idle(robot);
    }
    None
}

fn click_perf_button(robot: &cranpose::Robot, label: &str) {
    let Some(bounds) = find_button_exact_in_semantics(robot, label) else {
        fatal(robot, format!("button {label:?} not found"));
    };
    let (x, y) = center(bounds);
    robot.move_to(x, y).unwrap_or_else(|err| fatal(robot, err));
    robot
        .wait_for_present_frame()
        .unwrap_or_else(|err| fatal(robot, err));
    robot.click(x, y).unwrap_or_else(|err| fatal(robot, err));
    wait_for_perf_idle(robot);
}

fn prepare_perf_scenario(robot: &cranpose::Robot, scenario: PerfScenario) {
    if !scenario.requires_fetch() {
        return;
    }

    click_perf_button(robot, "Fetch");
    let Some(viewport_bounds) = wait_for_perf_text_bounds(robot, "MarkdownListViewport", 10_000)
    else {
        fatal(
            robot,
            "fetched Markdown viewer viewport did not appear before perf measurement",
        );
    };
    let (x, y) = center(viewport_bounds);
    robot.move_to(x, y).unwrap_or_else(|err| fatal(robot, err));
    robot
        .wait_for_present_frame()
        .unwrap_or_else(|err| fatal(robot, err));
    robot
        .reset_fps_stats()
        .unwrap_or_else(|err| fatal(robot, err));
}

fn perf_isolate_debug_enabled() -> bool {
    env_bool("CRANPOSE_PERF_LOG_ISOLATES", false)
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

#[derive(Debug, Default)]
struct FpsPacingAccumulator {
    samples: u64,
    worst_p95_ms: f32,
    worst_p99_ms: f32,
    worst_max_ms: f32,
    min_work_fps: f32,
    worst_work_p95_ms: f32,
    worst_work_max_ms: f32,
    max_missed_120hz_budget: u32,
    max_missed_60hz_budget: u32,
    max_stalled_50ms_frames: u32,
    max_work_missed_120hz_budget: u32,
    max_work_missed_60hz_budget: u32,
    max_work_stalled_50ms_frames: u32,
}

impl FpsPacingAccumulator {
    fn record(&mut self, stats: cranpose::FpsStats) {
        if stats.frame_count == 0 {
            return;
        }
        self.samples = self.samples.saturating_add(1);
        if stats.interval_count > 0 {
            self.worst_p95_ms = self.worst_p95_ms.max(stats.p95_ms);
            self.worst_p99_ms = self.worst_p99_ms.max(stats.p99_ms);
            self.worst_max_ms = self.worst_max_ms.max(stats.max_ms);
            self.max_missed_120hz_budget =
                self.max_missed_120hz_budget.max(stats.missed_120hz_budget);
            self.max_missed_60hz_budget = self.max_missed_60hz_budget.max(stats.missed_60hz_budget);
            self.max_stalled_50ms_frames =
                self.max_stalled_50ms_frames.max(stats.stalled_50ms_frames);
        }
        self.min_work_fps = if self.min_work_fps == 0.0 {
            stats.work_fps
        } else {
            self.min_work_fps.min(stats.work_fps)
        };
        self.worst_work_p95_ms = self.worst_work_p95_ms.max(stats.work_p95_ms);
        self.worst_work_max_ms = self.worst_work_max_ms.max(stats.work_max_ms);
        self.max_work_missed_120hz_budget = self
            .max_work_missed_120hz_budget
            .max(stats.work_missed_120hz_budget);
        self.max_work_missed_60hz_budget = self
            .max_work_missed_60hz_budget
            .max(stats.work_missed_60hz_budget);
        self.max_work_stalled_50ms_frames = self
            .max_work_stalled_50ms_frames
            .max(stats.work_stalled_50ms_frames);
    }
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
    let min_fps = fps_budget_from(
        std::env::var("CRANPOSE_PERF_MIN_FPS").ok().as_deref(),
        DEFAULT_MIN_FPS,
    );
    let max_p95_frame_ms = frame_ms_budget_from(
        std::env::var("CRANPOSE_PERF_MAX_P95_FRAME_MS")
            .ok()
            .as_deref(),
        DEFAULT_MAX_P95_FRAME_MS,
    );
    let max_50ms_stalls = env_u32("CRANPOSE_PERF_MAX_50MS_STALLS", u32::MAX);
    let wait_idle_after_drag = env_bool(
        "CRANPOSE_PERF_WAIT_IDLE_AFTER_DRAG",
        DEFAULT_WAIT_IDLE_AFTER_DRAG,
    );
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
    println!("Wait idle after drag: {wait_idle_after_drag}");
    if min_fps > 0.0 {
        println!("FPS budget: >{min_fps:.1}");
    }
    if max_p95_frame_ms > 0.0 {
        println!("P95 frame budget: <={max_p95_frame_ms:.2}ms");
    }
    if max_50ms_stalls != u32::MAX {
        println!("50ms stall budget: <= {max_50ms_stalls}");
    }

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
            prepare_perf_scenario(&robot, scenario);

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
            let mut pacing_stats = FpsPacingAccumulator::default();
            let mut measurement_started = false;
            let input_method = scenario.input_method();
            let start = Instant::now();

            if input_method == PerfInputMethod::Wheel {
                robot
                    .move_to(scenario.drag_x(), SCROLL_START_Y)
                    .unwrap_or_else(|err| fatal(&robot, err));
            }

            while start.elapsed() < total_duration {
                match input_method {
                    PerfInputMethod::Drag => {
                        let (from_y, to_y) = if direction_down {
                            (SCROLL_START_Y, SCROLL_END_Y)
                        } else {
                            (SCROLL_END_Y, SCROLL_START_Y)
                        };
                        if let Err(err) =
                            robot.drag(scenario.drag_x(), from_y, scenario.drag_x(), to_y)
                        {
                            fatal(&robot, err);
                        }
                    }
                    PerfInputMethod::Wheel => {
                        let delta_y = if direction_down {
                            -SCROLL_WHEEL_DELTA_Y
                        } else {
                            SCROLL_WHEEL_DELTA_Y
                        };
                        if let Err(err) = robot.mouse_scroll(0.0, delta_y) {
                            fatal(&robot, err);
                        }
                    }
                }
                direction_down = !direction_down;
                if wait_idle_after_drag {
                    wait_for_perf_idle(&robot);
                } else if input_method == PerfInputMethod::Wheel {
                    robot
                        .wait_for_present_frame()
                        .unwrap_or_else(|err| fatal(&robot, err));
                }

                let elapsed = start.elapsed();
                if !measurement_started && elapsed >= warmup_duration {
                    robot
                        .reset_fps_stats()
                        .unwrap_or_else(|err| fatal(&robot, err));
                    pacing_stats = FpsPacingAccumulator::default();
                    measurement_started = true;
                }

                if baseline_rss_kb.is_none() && measurement_started {
                    baseline_rss_kb = read_rss_kb();
                    if let Some(rss) = baseline_rss_kb {
                        peak_rss_kb = rss;
                    }
                }

                if measurement_started {
                    match robot.get_render_stats() {
                        Ok(Some(snapshot)) => render_stats.record(snapshot),
                        Ok(None) => {}
                        Err(err) => fatal(&robot, err),
                    }
                    let stats = robot.fps_stats().unwrap_or_else(|err| {
                        fatal(&robot, format!("failed to read FPS stats: {err}"))
                    });
                    pacing_stats.record(stats);
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
            print_render_summary(scenario.name(), render_stats);
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

            let stats = robot
                .fps_stats()
                .unwrap_or_else(|err| fatal(&robot, format!("failed to read FPS stats: {err}")));
            pacing_stats.record(stats);
            println!(
                "PERF_FPS_SUMMARY scenario={} fps={:.1} work_fps={:.1} avg_frame_ms={:.2} p95_frame_ms={:.2} p99_frame_ms={:.2} max_frame_ms={:.2} work_avg_ms={:.2} work_p95_ms={:.2} work_max_ms={:.2} missed_120hz={} missed_60hz={} stalls_50ms={} work_missed_120hz={} work_missed_60hz={} work_stalls_50ms={} worst_p95_frame_ms={:.2} worst_p99_frame_ms={:.2} worst_max_frame_ms={:.2} min_work_fps={:.1} worst_work_p95_ms={:.2} worst_work_max_ms={:.2} max_missed_120hz={} max_missed_60hz={} max_stalls_50ms={} max_work_missed_120hz={} max_work_missed_60hz={} max_work_stalls_50ms={} pacing_samples={} total_frames={} recompositions={} recompositions_per_second={}",
                scenario.name(),
                stats.fps,
                stats.work_fps,
                stats.avg_ms,
                stats.p95_ms,
                stats.p99_ms,
                stats.max_ms,
                stats.work_avg_ms,
                stats.work_p95_ms,
                stats.work_max_ms,
                stats.missed_120hz_budget,
                stats.missed_60hz_budget,
                stats.stalled_50ms_frames,
                stats.work_missed_120hz_budget,
                stats.work_missed_60hz_budget,
                stats.work_stalled_50ms_frames,
                pacing_stats.worst_p95_ms,
                pacing_stats.worst_p99_ms,
                pacing_stats.worst_max_ms,
                pacing_stats.min_work_fps,
                pacing_stats.worst_work_p95_ms,
                pacing_stats.worst_work_max_ms,
                pacing_stats.max_missed_120hz_budget,
                pacing_stats.max_missed_60hz_budget,
                pacing_stats.max_stalled_50ms_frames,
                pacing_stats.max_work_missed_120hz_budget,
                pacing_stats.max_work_missed_60hz_budget,
                pacing_stats.max_work_stalled_50ms_frames,
                pacing_stats.samples,
                stats.frame_count,
                stats.recompositions,
                stats.recomps_per_second,
            );
            if min_fps > 0.0 {
                println!(
                    "PERF_FPS_BUDGET scenario={} min_fps={:.1} observed_work_fps={:.1} observed_cadence_fps={:.1} passed={}",
                    scenario.name(),
                    min_fps,
                    pacing_stats.min_work_fps,
                    stats.fps,
                    pacing_stats.min_work_fps > min_fps,
                );
                if pacing_stats.min_work_fps <= min_fps {
                    fatal(
                        &robot,
                        format!(
                            "FPS budget failed for {}: expected work capacity >{:.1}, observed {:.1}",
                            scenario.name(),
                            min_fps,
                            pacing_stats.min_work_fps,
                        ),
                    );
                }
            }
            if max_p95_frame_ms > 0.0 {
                let observed_p95 = pacing_stats.worst_work_p95_ms;
                println!(
                    "PERF_FRAME_P95_BUDGET scenario={} max_p95_frame_ms={:.2} observed_work_p95_ms={:.2} observed_cadence_p95_ms={:.2} passed={}",
                    scenario.name(),
                    max_p95_frame_ms,
                    observed_p95,
                    pacing_stats.worst_p95_ms,
                    observed_p95 <= max_p95_frame_ms,
                );
                if observed_p95 > max_p95_frame_ms {
                    fatal(
                        &robot,
                        format!(
                            "P95 frame budget failed for {}: expected <={:.2}ms, observed {:.2}ms",
                            scenario.name(),
                            max_p95_frame_ms,
                            observed_p95,
                        ),
                    );
                }
            }
            if max_50ms_stalls != u32::MAX {
                println!(
                    "PERF_STALL_BUDGET scenario={} max_50ms_stalls={} observed_work_50ms_stalls={} observed_cadence_50ms_stalls={} passed={}",
                    scenario.name(),
                    max_50ms_stalls,
                    pacing_stats.max_work_stalled_50ms_frames,
                    pacing_stats.max_stalled_50ms_frames,
                    pacing_stats.max_work_stalled_50ms_frames <= max_50ms_stalls,
                );
                if pacing_stats.max_work_stalled_50ms_frames > max_50ms_stalls {
                    fatal(
                        &robot,
                        format!(
                            "50ms stall budget failed for {}: expected <= {}, observed {}",
                            scenario.name(),
                            max_50ms_stalls,
                            pacing_stats.max_work_stalled_50ms_frames,
                        ),
                    );
                }
            }
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
    use cranpose_render_wgpu::RenderStatsSnapshot;

    use super::{
        format_lazy_summary, fps_budget_from, frame_ms_budget_from, lazy_reuse_rate_pct,
        perf_robot_stats::RenderStatsAccumulator, timeout_budget_secs, timeout_slack_secs_from,
        FpsPacingAccumulator, LazyLayoutStats, PerfScenario, DEFAULT_MAX_P95_FRAME_MS,
        DEFAULT_MIN_FPS, DEFAULT_TIMEOUT_SLACK_SECS,
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
    fn fps_budget_accepts_only_positive_finite_values() {
        assert_eq!(fps_budget_from(None, DEFAULT_MIN_FPS), DEFAULT_MIN_FPS);
        assert_eq!(
            fps_budget_from(Some("invalid"), DEFAULT_MIN_FPS),
            DEFAULT_MIN_FPS
        );
        assert_eq!(fps_budget_from(Some("0"), DEFAULT_MIN_FPS), DEFAULT_MIN_FPS);
        assert_eq!(
            fps_budget_from(Some("-1"), DEFAULT_MIN_FPS),
            DEFAULT_MIN_FPS
        );
        assert_eq!(
            fps_budget_from(Some("inf"), DEFAULT_MIN_FPS),
            DEFAULT_MIN_FPS
        );
        assert_eq!(fps_budget_from(Some("120.5"), DEFAULT_MIN_FPS), 120.5);
    }

    #[test]
    fn frame_ms_budget_accepts_only_positive_finite_values() {
        assert_eq!(
            frame_ms_budget_from(None, DEFAULT_MAX_P95_FRAME_MS),
            DEFAULT_MAX_P95_FRAME_MS
        );
        assert_eq!(
            frame_ms_budget_from(Some("invalid"), DEFAULT_MAX_P95_FRAME_MS),
            DEFAULT_MAX_P95_FRAME_MS
        );
        assert_eq!(
            frame_ms_budget_from(Some("0"), DEFAULT_MAX_P95_FRAME_MS),
            DEFAULT_MAX_P95_FRAME_MS
        );
        assert_eq!(
            frame_ms_budget_from(Some("nan"), DEFAULT_MAX_P95_FRAME_MS),
            DEFAULT_MAX_P95_FRAME_MS
        );
        assert_eq!(
            frame_ms_budget_from(Some("8.33"), DEFAULT_MAX_P95_FRAME_MS),
            8.33
        );
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
            PerfScenario::parse("markdown_scroll"),
            Some(PerfScenario::MarkdownScroll)
        );
        assert_eq!(
            PerfScenario::parse("markdown_viewer_scroll"),
            Some(PerfScenario::MarkdownViewerScroll)
        );
        assert_eq!(
            PerfScenario::parse("markdown_default_viewer_scroll"),
            Some(PerfScenario::MarkdownDefaultViewerScroll)
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
    fn markdown_perf_scenarios_use_wheel_input_path() {
        assert_eq!(
            PerfScenario::MarkdownScroll.input_method(),
            super::PerfInputMethod::Wheel
        );
        assert_eq!(
            PerfScenario::MarkdownViewerScroll.input_method(),
            super::PerfInputMethod::Wheel
        );
        assert_eq!(
            PerfScenario::MarkdownDefaultViewerScroll.input_method(),
            super::PerfInputMethod::Wheel
        );
        assert_eq!(
            PerfScenario::LazyListScroll.input_method(),
            super::PerfInputMethod::Drag
        );
    }

    #[test]
    fn only_fetched_markdown_perf_requires_fetch_setup() {
        assert!(!PerfScenario::MarkdownScroll.requires_fetch());
        assert!(PerfScenario::MarkdownViewerScroll.requires_fetch());
        assert!(PerfScenario::MarkdownDefaultViewerScroll.requires_fetch());
        assert!(!PerfScenario::LazyListScroll.requires_fetch());
    }

    #[test]
    fn fps_pacing_accumulator_tracks_worst_rolling_samples() {
        let mut accumulator = FpsPacingAccumulator::default();

        accumulator.record(cranpose::FpsStats {
            frame_count: 8,
            interval_count: 3,
            p95_ms: 9.0,
            p99_ms: 11.0,
            max_ms: 18.0,
            work_fps: 160.0,
            work_p95_ms: 6.0,
            work_max_ms: 7.0,
            missed_120hz_budget: 2,
            missed_60hz_budget: 1,
            stalled_50ms_frames: 0,
            work_missed_120hz_budget: 0,
            work_missed_60hz_budget: 0,
            work_stalled_50ms_frames: 0,
            ..Default::default()
        });
        accumulator.record(cranpose::FpsStats {
            frame_count: 12,
            interval_count: 3,
            p95_ms: 7.0,
            p99_ms: 20.0,
            max_ms: 51.0,
            work_fps: 140.0,
            work_p95_ms: 9.0,
            work_max_ms: 12.0,
            missed_120hz_budget: 1,
            missed_60hz_budget: 2,
            stalled_50ms_frames: 1,
            work_missed_120hz_budget: 1,
            work_missed_60hz_budget: 0,
            work_stalled_50ms_frames: 0,
            ..Default::default()
        });

        assert_eq!(accumulator.samples, 2);
        assert_eq!(accumulator.worst_p95_ms, 9.0);
        assert_eq!(accumulator.worst_p99_ms, 20.0);
        assert_eq!(accumulator.worst_max_ms, 51.0);
        assert_eq!(accumulator.min_work_fps, 140.0);
        assert_eq!(accumulator.worst_work_p95_ms, 9.0);
        assert_eq!(accumulator.worst_work_max_ms, 12.0);
        assert_eq!(accumulator.max_missed_120hz_budget, 2);
        assert_eq!(accumulator.max_missed_60hz_budget, 2);
        assert_eq!(accumulator.max_stalled_50ms_frames, 1);
        assert_eq!(accumulator.max_work_missed_120hz_budget, 1);
        assert_eq!(accumulator.max_work_missed_60hz_budget, 0);
        assert_eq!(accumulator.max_work_stalled_50ms_frames, 0);
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
