//! The Performance tab: the nightly comparison of the latest release against
//! main, and Cranpose against other frameworks, read from the `perf-data`
//! branch that `scripts/perf/publish.py` writes.

use std::{cmp::Ordering, collections::BTreeMap, rc::Rc};

use cranpose_core::{self, MutableState};
use cranpose_services::{isSystemInDarkTheme, local_http_client, HttpClientRef};
use cranpose_ui::{
    composable,
    text::FontWeight,
    widgets::{FlowRow, FlowSpec},
    Box, BoxSpec, Brush, Button, ButtonSpec, Color, Column, ColumnSpec, DrawStyle,
    LinearArrangement, Modifier, Path, Point, Row, RowSpec, ScrollState, Stroke, Text,
    VerticalAlignment,
};

use super::demo_text::text_style;

mod chart_view;
mod curve_plot;
mod focus;
mod launch_chart;
mod motion;
mod plot;
mod runs;
mod subject_bars;
mod table;
use focus::Focus;
use launch_chart::{launch_lines, LaunchChart};
use plot::{Axis, Plot};
pub use runs::{
    LegSamples, PerfFramesLeg, PerfFramesScenario, PerfIndex, PerfProtocol, PerfRun, PerfRunFrames,
    PerfScenario, PerfStart, PerfSubject, STARTS,
};
use subject_bars::SubjectBars;
use table::NightlyTable;

/// The run index the nightly publishes, served as raw JSON from the branch.
const INDEX_URL: &str =
    "https://raw.githubusercontent.com/samoylenkodmitry/Cranpose/perf-data/index.json";
/// The data branch's files, a run's own among them.
const DATA_URL: &str = "https://raw.githubusercontent.com/samoylenkodmitry/Cranpose/perf-data";
/// Where a framework's gauntlet source opens.
const SOURCE_URL: &str = "https://github.com/samoylenkodmitry/Cranpose/blob";

/// Where the charts read a run's frames: the data branch, or run files at
/// hand, by their path on the branch.
#[derive(Clone)]
pub enum FrameSource {
    Http(HttpClientRef),
    Files(Rc<BTreeMap<String, String>>),
}

impl PartialEq for FrameSource {
    /// The same client, or the same files.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Http(a), Self::Http(b)) => std::sync::Arc::ptr_eq(a, b),
            (Self::Files(a), Self::Files(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// How often the launch chart reads the frame rate, in milliseconds.
const CHART_STEP_MS: f64 = 50.0;
/// The span the frame rate at a moment is taken over, in milliseconds.
const RATE_WINDOW_MS: f64 = 500.0;

/// The frame rate at every `step_ms` from the launch to `run_ms`, from the
/// times frames were presented, in milliseconds since the launch: the frames
/// presented in the last 500 ms, per second. It is zero before the first
/// frame and after a stall as long as the window, and never reads more than
/// the frames shown.
pub fn launch_fps(frame_ms: &[f64], run_ms: f64, step_ms: f64) -> Vec<f64> {
    let steps = (run_ms / step_ms).floor() as usize + 1;
    let mut rates = Vec::with_capacity(steps);
    let (mut shown, mut oldest) = (0, 0);
    for step in 0..steps {
        let now = step as f64 * step_ms;
        while shown < frame_ms.len() && frame_ms[shown] <= now {
            shown += 1;
        }
        while oldest < shown && frame_ms[oldest] <= now - RATE_WINDOW_MS {
            oldest += 1;
        }
        rates.push((shown - oldest) as f64 * 1000.0 / RATE_WINDOW_MS);
    }
    rates
}

#[derive(Clone, Debug, PartialEq)]
enum LoadState {
    Loading,
    Loaded(Rc<PerfIndex>),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq)]
struct Palette {
    background: Color,
    surface: Color,
    text: Color,
    muted: Color,
    accent: Color,
    baseline: Color,
    better: Color,
    worse: Color,
    open: Color,
}

impl Palette {
    fn new(dark: bool) -> Self {
        if dark {
            Self {
                background: Color::from_rgb_u8(0x11, 0x14, 0x1B),
                surface: Color::from_rgb_u8(0x1C, 0x21, 0x2B),
                text: Color::from_rgb_u8(0xE5, 0xE7, 0xEB),
                muted: Color::from_rgb_u8(0x9C, 0xA3, 0xAF),
                accent: Color::from_rgb_u8(0x60, 0xA5, 0xFA),
                baseline: Color::from_rgb_u8(0x6B, 0x72, 0x80),
                better: Color::from_rgb_u8(0x4A, 0xDE, 0x80),
                worse: Color::from_rgb_u8(0xF8, 0x71, 0x71),
                open: Color::from_rgb_u8(0xFB, 0xBF, 0x24),
            }
        } else {
            Self {
                background: Color::from_rgb_u8(0xF3, 0xF4, 0xF6),
                surface: Color::WHITE,
                text: Color::from_rgb_u8(0x11, 0x18, 0x27),
                muted: Color::from_rgb_u8(0x6B, 0x72, 0x80),
                accent: Color::from_rgb_u8(0x25, 0x63, 0xEB),
                baseline: Color::from_rgb_u8(0x9C, 0xA3, 0xAF),
                better: Color::from_rgb_u8(0x16, 0xA3, 0x4A),
                worse: Color::from_rgb_u8(0xDC, 0x26, 0x26),
                open: Color::from_rgb_u8(0xD9, 0x77, 0x06),
            }
        }
    }

    fn verdict(&self, verdict: Option<&String>) -> Color {
        match verdict.map(String::as_str) {
            Some("better") => self.better,
            Some("worse") => self.worse,
            Some("inconclusive") => self.open,
            _ => self.text,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Selection {
    scenario: String,
    start: &'static str,
    metric: Metric,
}

/// A median to one decimal, or a dash where the run has none.
fn figure(value: Option<f64>) -> String {
    value.map_or_else(|| "–".to_string(), |value| format!("{value:.1}"))
}

/// Where a metric's chart reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Curve {
    /// The frame rate, from the times of the frames the display showed.
    Frames,
    /// The run's samples of the metric under this key.
    Samples(&'static str),
    /// No curve, and why.
    None(&'static str),
}

/// What the trend and the framework bars show of each run's medians.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Metric {
    Fps,
    FirstFrame,
    CpuPerFrame,
    CpuCores,
    Ram,
    GpuRam,
    CpuClock,
    GpuClock,
}

impl Metric {
    const ALL: [Self; 8] = [
        Self::Fps,
        Self::FirstFrame,
        Self::CpuPerFrame,
        Self::CpuCores,
        Self::Ram,
        Self::GpuRam,
        Self::CpuClock,
        Self::GpuClock,
    ];

    /// The median's name in the run index.
    fn key(self) -> &'static str {
        match self {
            Self::Fps => "fps",
            Self::FirstFrame => "first_frame_s",
            Self::CpuPerFrame => "cpu_ms_per_frame",
            Self::CpuCores => "cpu_cores",
            Self::Ram => "ram_mb",
            Self::GpuRam => "gpu_ram_mb",
            Self::CpuClock => "cpu_mhz",
            Self::GpuClock => "gpu_mhz",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Fps => "fps",
            Self::FirstFrame => "first frame ms",
            Self::CpuPerFrame => "CPU ms per frame",
            Self::CpuCores => "CPU cores",
            Self::Ram => "RAM MB",
            Self::GpuRam => "GPU RAM MB",
            Self::CpuClock => "CPU MHz",
            Self::GpuClock => "GPU MHz",
        }
    }

    /// How each platform measures it.
    fn about(self) -> &'static str {
        match self {
            Self::Fps => {
                "Frames the display showed per second over the whole run from the launch, counted \
                 outside the app; 60 at most."
            }
            Self::FirstFrame => {
                "Milliseconds from the launch command to the first frame the display showed."
            }
            Self::CpuPerFrame => {
                "CPU time of the app and every process it started, per frame shown; on macOS \
                 the window server's too."
            }
            Self::CpuCores => {
                "Cores the app and every process it started kept busy: their CPU seconds per \
                 second of the run."
            }
            Self::Ram => {
                "Memory the app holds at the run's end: PSS on Android, the footprint of the \
                 app and every process it started on macOS. The chart's curve on Android is the \
                 resident memory (RSS), which counts shared pages whole: PSS costs the app a \
                 garbage collection to read."
            }
            Self::GpuRam => {
                "The GPU's part of that memory: GL and EGL mtrack on Android; Metal buffers, \
                 textures and window surfaces on macOS. Read once, at the run's end."
            }
            Self::CpuClock => {
                "Mean clock over the run of the big cores on Android, the performance cores on \
                 macOS."
            }
            Self::GpuClock => "Mean GPU clock over the run.",
        }
    }

    fn curve_unit(self) -> &'static str {
        match self {
            Self::Fps | Self::FirstFrame => "fps",
            Self::CpuPerFrame => "ms/frame",
            Self::CpuCores => "cores",
            Self::Ram | Self::GpuRam => "MB",
            Self::CpuClock | Self::GpuClock => "MHz",
        }
    }

    fn curve_format(self, value: f64) -> String {
        match self {
            Self::FirstFrame => Self::Fps.format(value),
            _ => self.format(value),
        }
    }

    fn format(self, value: f64) -> String {
        match self {
            Self::Fps | Self::CpuPerFrame => format!("{value:.1}"),
            Self::CpuCores => format!("{value:.2}"),
            // Medians in seconds, shown in milliseconds.
            Self::FirstFrame => format!("{:.0}", value * 1000.0),
            Self::Ram | Self::GpuRam | Self::CpuClock | Self::GpuClock => format!("{value:.0}"),
        }
    }

    /// Where its chart reads it: the frames the display showed, the run's
    /// samples by their key, or nowhere, with why.
    fn curve(self) -> Curve {
        match self {
            Self::Fps | Self::FirstFrame => Curve::Frames,
            Self::CpuPerFrame => Curve::Samples("cpu_ms_per_frame"),
            Self::CpuCores => Curve::Samples("cores"),
            Self::Ram => Curve::Samples("ram_mb"),
            Self::CpuClock => Curve::Samples("cpu_mhz"),
            Self::GpuClock => Curve::Samples("gpu_mhz"),
            Self::GpuRam => Curve::None(
                "No curve: the GPU's memory is read once, at the run's end. On Android, \
                 reading it costs the app a garbage collection.",
            ),
        }
    }

    /// Orders two values best first: the higher frame rate, the lower of the
    /// rest, which cost time, memory or power; a value before none.
    fn best_first(self, a: Option<f64>, b: Option<f64>) -> Ordering {
        match (a, b) {
            (Some(a), Some(b)) if self == Self::Fps => b.total_cmp(&a),
            (Some(a), Some(b)) => a.total_cmp(&b),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }

    /// What a full bar stands for: the display's 60 fps for the frame rate,
    /// the run's largest value for the rest.
    fn full_scale(self, values: &[Option<f64>]) -> f64 {
        match self {
            Self::Fps => 60.0,
            _ => values.iter().flatten().copied().fold(0.0, f64::max),
        }
    }
}

async fn load_index(client: &HttpClientRef) -> Result<PerfIndex, String> {
    let json = client
        .get_text(INDEX_URL)
        .await
        .map_err(|error| format!("The run index did not load: {error}"))?;
    PerfIndex::parse(&json)
}

/// The tab: loads the run index when it opens and on Refresh.
#[composable]
pub fn PerformanceTab() {
    let state = cranpose_core::rememberMutableStateOf(|| LoadState::Loading);
    let refresh = cranpose_core::rememberMutableStateOf(|| 0u64);
    let client = local_http_client().current();
    cranpose_core::LaunchedEffect(refresh.get(), async move |scope| {
        state.set(LoadState::Loading);
        let client = std::sync::Arc::clone(&client);
        scope.launch_background(
            move |_token| async move { load_index(&client).await },
            move |result| {
                state.set(match result {
                    Ok(index) => LoadState::Loaded(Rc::new(index)),
                    Err(error) => LoadState::Failed(error),
                });
            },
        );
    });
    let palette = Palette::new(isSystemInDarkTheme());
    let frames = FrameSource::Http(local_http_client().current());
    match state.get() {
        LoadState::Loaded(index) => PerformanceDashboard(index, Some(refresh), frames),
        LoadState::Loading => Message(palette, "Loading the nightly runs…".to_string()),
        LoadState::Failed(error) => Message(palette, error),
    }
}

#[composable]
fn Message(palette: Palette, text: String) {
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(palette.background)
            .padding(24.0),
        ColumnSpec::default(),
        move || {
            Text(
                text.clone(),
                Modifier::empty(),
                text_style(15.0, palette.muted, None),
            );
        },
    );
}

/// Everything the tab shows for one run index, with each run's frames from
/// `source`. `refresh` adds a Refresh button that bumps it.
#[composable]
pub fn PerformanceDashboard(
    index: Rc<PerfIndex>,
    refresh: Option<MutableState<u64>>,
    source: FrameSource,
) {
    let palette = Palette::new(isSystemInDarkTheme());
    let scroll = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
    // The nightly's scenario, which its chart and its trend share.
    let scenario = cranpose_core::rememberMutableStateOf(|| "gauntlet".to_string());
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(palette.background)
            .vertical_scroll(scroll, false)
            .padding(12.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
        move || {
            Header(palette, refresh);
            let nightly: Vec<PerfRun> = index.nightly().into_iter().cloned().collect();
            match nightly.last() {
                Some(latest) => {
                    LatestNightly(palette, Rc::new(latest.clone()), source.clone(), scenario);
                    Trend(palette, Rc::new(nightly.clone()), scenario);
                }
                None => {
                    Text(
                        "No nightly run has been published yet.",
                        Modifier::empty(),
                        text_style(14.0, palette.muted, None),
                    );
                }
            }
            for run in index.latest_framework_runs() {
                Frameworks(palette, Rc::new(run.clone()), source.clone());
            }
        },
    );
}

/// The tab's title, how every chart and bar reads, and Refresh.
#[composable]
fn Header(palette: Palette, refresh: Option<MutableState<u64>>) {
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding_symmetric(4.0, 4.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
            .vertical_alignment(VerticalAlignment::Top),
        move || {
            Column(
                Modifier::empty().weight(1.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                move || {
                    Text(
                        "Performance",
                        Modifier::empty(),
                        text_style(22.0, palette.text, Some(FontWeight::BOLD)),
                    );
                    Text(
                        "Measured from outside each app, from its launch, over the same span \
                         for every framework. First start: the first launch after install. \
                         Second start: the launch after it.",
                        Modifier::empty(),
                        text_style(13.0, palette.muted, None),
                    );
                    Text(
                        format!(
                            "Charts: the chosen metric from the launch, the frame rate over the \
                             last {RATE_WINDOW_MS:.0} ms and the rest as sampled every half \
                             second; a line is the median launch, its band the lowest to the \
                             highest. Bars: medians over the run, best first. Hover a line, a \
                             bar or a name to follow one framework; click bars to chart \
                             only them."
                        ),
                        Modifier::empty(),
                        text_style(12.0, palette.muted, None),
                    );
                },
            );
            if let Some(refresh) = refresh {
                Button(
                    Modifier::empty()
                        .background(palette.accent)
                        .rounded_corners(8.0)
                        .padding_symmetric(12.0, 6.0),
                    ButtonSpec::default(),
                    move || refresh.update(|value| *value = value.wrapping_add(1)),
                    move || {
                        Text(
                            "Refresh",
                            Modifier::empty(),
                            text_style(13.0, Color::WHITE, Some(FontWeight::BOLD)),
                        );
                    },
                );
            }
        },
    );
}

#[composable]
fn Card(palette: Palette, title: String, content: impl Fn() + 'static) {
    Column(
        Modifier::empty()
            .fill_max_width()
            .background(palette.surface)
            .rounded_corners(14.0)
            .padding(14.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
        move || {
            Text(
                title.clone(),
                Modifier::empty(),
                text_style(16.0, palette.text, Some(FontWeight::BOLD)),
            );
            content();
        },
    );
}

/// `chosen` when the run has it, else its first scenario.
fn shown_scenario(run: &PerfRun, chosen: &str) -> String {
    if run.scenarios.contains_key(chosen) {
        chosen.to_string()
    } else {
        run.scenarios.keys().next().cloned().unwrap_or_default()
    }
}

/// The latest night: each scenario's medians on both builds for the start
/// chosen, and the chosen scenario's launches.
#[composable]
fn LatestNightly(
    palette: Palette,
    run: Rc<PerfRun>,
    source: FrameSource,
    scenario: MutableState<String>,
) {
    let start = cranpose_core::rememberMutableStateOf(|| STARTS[0].0);
    let release = run.release.clone().unwrap_or_default();
    let main = run.main.clone().unwrap_or_default();
    let title = format!(
        "Nightly · {release} against main {} · {} · {}",
        &main[..main.len().min(9)],
        run.device,
        run.date()
    );
    let subjects: Rc<Vec<(usize, PerfSubject)>> =
        Rc::new(run.subjects.iter().cloned().enumerate().collect());
    Card(palette, title, move || {
        StartChips(palette, start);
        NightlyTable(palette, Rc::clone(&run), start.get());
        Text(
            "Main's median over the release's. Green: main is better; red: worse; amber: not \
             settled within four pairs of legs; ⚠: a second run confirmed it.",
            Modifier::empty(),
            text_style(12.0, palette.muted, None),
        );
        RunView(
            palette,
            Rc::clone(&run),
            Rc::clone(&subjects),
            source.clone(),
            scenario,
            start.get(),
            Metric::Fps,
        );
    });
}

/// Every night's medians of the chosen scenario, start and metric, the
/// release's and main's.
#[composable]
fn Trend(palette: Palette, nightly: Rc<Vec<PerfRun>>, scenario: MutableState<String>) {
    let metric = cranpose_core::rememberMutableStateOf(|| Metric::Fps);
    let start = cranpose_core::rememberMutableStateOf(|| STARTS[0].0);
    let title = format!(
        "{}: {}, night by night",
        scenario.get(),
        metric.get().name()
    );
    Card(palette, title, move || {
        let names: Vec<String> = nightly
            .last()
            .map(|run| run.scenarios.keys().cloned().collect())
            .unwrap_or_default();
        ScenarioChips(palette, names, scenario);
        StartChips(palette, start);
        MetricChips(palette, metric);
        let (shown, measured, start) = (scenario.get(), metric.get(), start.get());
        let series = |subject: usize| -> Vec<f32> {
            nightly
                .iter()
                .map(|run| {
                    run.median(&shown, start, subject, measured.key())
                        .unwrap_or(f64::NAN) as f32
                })
                .collect()
        };
        let regressed: Vec<bool> = nightly
            .iter()
            .map(|run| {
                run.start(&shown, start)
                    .is_some_and(|entry| !entry.confirmed.is_empty())
            })
            .collect();
        let dates = nightly.iter().map(|run| run.date().to_string()).collect();
        LineChart(palette, dates, series(0), series(1), regressed);
        Row(
            Modifier::empty().fill_max_width(),
            RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(14.0)),
            move || {
                for (label, color) in [
                    ("release", palette.baseline),
                    ("main", palette.accent),
                    ("confirmed regression", palette.worse),
                ] {
                    Swatch(palette, label, color);
                }
            },
        );
        Text(
            measured.about(),
            Modifier::empty(),
            text_style(12.0, palette.muted, None),
        );
    });
}

/// A coloured dot and its name.
#[composable]
fn Swatch(palette: Palette, label: &'static str, color: Color) {
    Row(
        Modifier::empty(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(6.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Dot(color);
            Text(
                label,
                Modifier::empty(),
                text_style(12.0, palette.muted, None),
            );
        },
    );
}

/// The colour a subject's line has on the charts.
#[composable]
fn Dot(color: Color) {
    Box(
        Modifier::empty()
            .size_points(10.0, 10.0)
            .rounded_corners(5.0)
            .background(color),
        BoxSpec::default(),
        || {},
    );
}

/// The starts, the one shown highlighted.
#[composable]
fn StartChips(palette: Palette, start: MutableState<&'static str>) {
    let shown = start.get();
    Chips(
        palette,
        STARTS.map(|(_, title)| title.to_string()).to_vec(),
        STARTS.iter().position(|(name, _)| *name == shown),
        move |index| start.set(STARTS[index].0),
    );
}

/// A run's scenarios, the one shown highlighted; none for a run of one.
#[composable]
fn ScenarioChips(palette: Palette, names: Vec<String>, scenario: MutableState<String>) {
    if names.len() < 2 {
        return;
    }
    let chosen = names.iter().position(|name| *name == scenario.get());
    let choices = names.clone();
    Chips(palette, names, chosen, move |index| {
        scenario.set(choices[index].clone());
    });
}

/// The metrics, the one shown highlighted.
#[composable]
fn MetricChips(palette: Palette, metric: MutableState<Metric>) {
    let shown = metric.get();
    Chips(
        palette,
        Metric::ALL.map(|metric| metric.name().to_string()).to_vec(),
        Metric::ALL.iter().position(|candidate| *candidate == shown),
        move |index| metric.set(Metric::ALL[index]),
    );
}

/// A row of choices that wraps where the card is narrow, the `chosen` one
/// highlighted.
#[composable]
fn Chips(
    palette: Palette,
    names: Vec<String>,
    chosen: Option<usize>,
    choose: impl Fn(usize) + 'static,
) {
    let choose: Rc<dyn Fn(usize)> = Rc::new(choose);
    FlowRow(
        Modifier::empty().fill_max_width(),
        FlowSpec::default()
            .main_axis_spacing(6.0)
            .cross_axis_spacing(6.0),
        move || {
            for (index, name) in names.iter().enumerate() {
                let highlighted = chosen == Some(index);
                let choose = Rc::clone(&choose);
                Button(
                    Modifier::empty()
                        .background(if highlighted {
                            palette.accent
                        } else {
                            palette.background
                        })
                        .rounded_corners(14.0)
                        .padding_symmetric(12.0, 5.0),
                    ButtonSpec::default(),
                    move || choose(index),
                    {
                        let name = name.clone();
                        move || {
                            Text(
                                name.clone(),
                                Modifier::empty(),
                                text_style(
                                    12.0,
                                    if highlighted {
                                        Color::WHITE
                                    } else {
                                        palette.text
                                    },
                                    highlighted.then_some(FontWeight::BOLD),
                                ),
                            );
                        }
                    },
                );
            }
        },
    );
}

/// The release's and main's value night by night, `dates` under the first
/// and the last, a confirmed regression marked on main's.
#[composable]
fn LineChart(
    palette: Palette,
    dates: Vec<String>,
    release: Vec<f32>,
    main: Vec<f32>,
    regressed: Vec<bool>,
) {
    let peak = release
        .iter()
        .chain(&main)
        .copied()
        .filter(|value| value.is_finite())
        .fold(0.0f32, f32::max);
    let axis = Axis::from_zero(f64::from(peak));
    Plot(palette, axis, move |scope| {
        let size = scope.size();
        let count = main.len();
        let at = |index: usize, value: f32| Point {
            x: if count > 1 {
                TREND_INSET + index as f32 * (size.width - 2.0 * TREND_INSET) / (count - 1) as f32
            } else {
                size.width / 2.0
            },
            y: axis.y(size.height, f64::from(value)),
        };
        for series in [(&release, palette.baseline), (&main, palette.accent)] {
            let mut line = Path::new();
            let mut started = false;
            for (index, value) in series.0.iter().enumerate() {
                if !value.is_finite() {
                    started = false;
                    continue;
                }
                if started {
                    line.line_to(at(index, *value));
                } else {
                    line.move_to(at(index, *value));
                    started = true;
                }
                scope.draw_circle(Brush::solid(series.1), at(index, *value), 3.5);
            }
            scope.draw_path(
                &line,
                Brush::solid(series.1),
                DrawStyle::Stroke(Stroke::new(2.0)),
            );
        }
        for (index, value) in main.iter().enumerate() {
            if regressed.get(index) == Some(&true) && value.is_finite() {
                scope.draw_circle(Brush::solid(palette.worse), at(index, *value), 5.0);
            }
        }
    });
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().horizontal_arrangement(if dates.len() > 1 {
            LinearArrangement::SpaceBetween
        } else {
            LinearArrangement::Center
        }),
        move || {
            let ends = dates
                .first()
                .into_iter()
                .chain(dates.last().filter(|_| dates.len() > 1));
            for date in ends {
                Text(
                    date.clone(),
                    Modifier::empty(),
                    text_style(11.0, palette.muted, None),
                );
            }
        },
    );
}

/// How far the first and the last night sit from a trend chart's sides.
const TREND_INSET: f32 = 6.0;

/// A run's frames as a card has them: loading, loaded, failed, or never
/// kept, by a run measured after a warm-up.
#[derive(Clone, Debug, PartialEq)]
enum FramesState {
    Loading,
    Loaded(Rc<PerfRunFrames>),
    Failed(String),
}

/// Loads a run's own `file` from `source` once, for every chart of its card.
#[composable]
fn run_frames(source: FrameSource, file: String) -> MutableState<FramesState> {
    let state = cranpose_core::rememberMutableStateOf(|| FramesState::Loading);
    cranpose_core::LaunchedEffect(file.clone(), async move |scope| {
        if file.is_empty() {
            state.set(FramesState::Failed(
                "The run names no file of its frames.".to_string(),
            ));
            return;
        }
        match source.clone() {
            FrameSource::Files(files) => state.set(match files.get(&file) {
                Some(json) => loaded(PerfRunFrames::parse(json)),
                None => FramesState::Failed(format!("No frames for {file}.")),
            }),
            FrameSource::Http(client) => {
                let url = format!("{DATA_URL}/{file}");
                scope.launch_background(
                    move |_token| async move {
                        let json = client
                            .get_text(&url)
                            .await
                            .map_err(|error| format!("The run's frames did not load: {error}"))?;
                        PerfRunFrames::parse(&json)
                    },
                    move |result| state.set(loaded(result)),
                );
            }
        }
    });
    state
}

fn loaded(result: Result<PerfRunFrames, String>) -> FramesState {
    match result {
        Ok(frames) => FramesState::Loaded(Rc::new(frames)),
        Err(error) => FramesState::Failed(error),
    }
}

/// A run's scenarios to choose from, then the chosen one's `start`: every
/// subject's `metric` since the launch, from the run's own file at `source`,
/// then the metric's median as a bar in its line's colour.
#[composable]
fn RunView(
    palette: Palette,
    run: Rc<PerfRun>,
    subjects: Rc<Vec<(usize, PerfSubject)>>,
    source: FrameSource,
    scenario: MutableState<String>,
    start: &'static str,
    metric: Metric,
) {
    let focus = Focus::remember();
    ScenarioChips(palette, run.scenarios.keys().cloned().collect(), scenario);
    let frames = run_frames(source, run.file.clone()).get();
    let selection = Selection {
        scenario: shown_scenario(&run, &scenario.get()),
        start,
        metric,
    };
    let lines = launch_lines(palette, &run, &selection.scenario, start, &subjects);
    LaunchChart(
        palette,
        focus,
        frames,
        run_ms(&run),
        selection.clone(),
        lines.clone(),
    );
    SubjectBars(palette, focus, run, subjects, selection, lines);
}

/// The run's length in milliseconds from its launch.
fn run_ms(run: &PerfRun) -> f64 {
    run.protocol.run_s.unwrap_or(10.0) * 1000.0
}

/// A device's latest framework comparison: the chosen start and metric of
/// the chosen scenario, one chart and every framework's bar.
#[composable]
fn Frameworks(palette: Palette, run: Rc<PerfRun>, source: FrameSource) {
    // Alphabetical: each keeps its line's colour whatever the bars' order.
    let mut subjects: Vec<(usize, PerfSubject)> =
        run.subjects.iter().cloned().enumerate().collect();
    subjects.sort_by(|(_, a), (_, b)| a.name.cmp(&b.name));
    let subjects = Rc::new(subjects);
    let metric = cranpose_core::rememberMutableStateOf(|| Metric::Fps);
    let start = cranpose_core::rememberMutableStateOf(|| STARTS[0].0);
    let scenario = cranpose_core::rememberMutableStateOf(String::new);
    let place = if run.kind == "browser" {
        "Frameworks in the browser"
    } else {
        "Frameworks"
    };
    let title = format!("{place} · {} · {}", run.device, run.date());
    Card(palette, title, move || {
        StartChips(palette, start);
        MetricChips(palette, metric);
        RunView(
            palette,
            Rc::clone(&run),
            Rc::clone(&subjects),
            source.clone(),
            scenario,
            start.get(),
            metric.get(),
        );
        Text(
            metric.get().about(),
            Modifier::empty(),
            text_style(12.0, palette.muted, None),
        );
    });
}
