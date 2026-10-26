//! The Performance tab: the nightly comparison of the latest release against
//! main, and Cranpose against other frameworks, read from the `perf-data`
//! branch that `scripts/perf/publish.py` writes.

use std::{cmp::Ordering, collections::BTreeMap, rc::Rc};

use cranpose_core::{self, MutableState};
use cranpose_services::{isSystemInDarkTheme, local_http_client, local_uri_handler, HttpClientRef};
use cranpose_ui::{
    composable,
    text::FontWeight,
    widgets::{FlowRow, FlowSpec},
    Alignment, Box, BoxSpec, Brush, Button, ButtonSpec, Canvas, Color, Column, ColumnSpec,
    DrawScope, DrawStyle, LinearArrangement, Modifier, Path, Point, Row, RowSpec, ScrollState,
    Stroke, Text, TextOptions, TextOverflow, TextWithOptions, VerticalAlignment,
};
use serde::Deserialize;

use super::demo_text::text_style;

mod table;
use table::NightlyTable;

/// The run index the nightly publishes, served as raw JSON from the branch.
const INDEX_URL: &str =
    "https://raw.githubusercontent.com/samoylenkodmitry/Cranpose/perf-data/index.json";
/// The data branch's files, a run's own among them.
const DATA_URL: &str = "https://raw.githubusercontent.com/samoylenkodmitry/Cranpose/perf-data";
/// Where a framework's gauntlet source opens.
const SOURCE_URL: &str = "https://github.com/samoylenkodmitry/Cranpose/blob";

/// `index.json` on the perf-data branch.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PerfIndex {
    pub runs: Vec<PerfRun>,
}

/// One comparison: two builds on one device, scenario by scenario.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PerfRun {
    /// The run's own file on the branch, which holds every leg's frames.
    #[serde(default)]
    pub file: String,
    pub kind: String,
    pub started_at: String,
    pub device: String,
    #[serde(default)]
    pub main: Option<String>,
    #[serde(default)]
    pub release: Option<String>,
    pub subjects: Vec<PerfSubject>,
    pub scenarios: BTreeMap<String, PerfScenario>,
    #[serde(default)]
    pub protocol: PerfProtocol,
}

/// How a run measured each launch.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct PerfProtocol {
    /// Seconds each launch was measured for, from its start.
    #[serde(default)]
    pub run_s: Option<f64>,
    /// The starts each round measured of every subject: its first after its
    /// install and its second. An older run did not keep them apart, and the
    /// dashboard leaves it out.
    #[serde(default)]
    pub starts: Vec<String>,
}

/// A run's own file: every leg, with the time of each frame it presented.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PerfRunFrames {
    pub scenarios: Vec<PerfFramesScenario>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PerfFramesScenario {
    pub scenario: String,
    pub legs: Vec<PerfFramesLeg>,
}

/// One launch of one subject.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PerfFramesLeg {
    pub subject: String,
    /// Which start of its round the launch was: `first`, after the subject's
    /// data was cleared as its install leaves it, or `second`.
    #[serde(default)]
    pub start: String,
    /// When each frame was presented, in milliseconds since the launch.
    #[serde(default)]
    pub frame_ms: Vec<f64>,
    /// Another process spent more than the run allows in this leg: it is
    /// shown, and left out of the medians.
    #[serde(default)]
    pub disturbed: bool,
    /// The run at each of the sampler's intervals; none in a run before the
    /// sampler.
    #[serde(default)]
    pub samples: LegSamples,
}

/// A launch sampled at each interval of its run, each sample standing for
/// the interval since the one before; the first since the launch.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct LegSamples {
    /// When each sample was taken, in milliseconds since the launch.
    #[serde(default)]
    pub t_ms: Vec<f64>,
    /// Each metric's value at each sample, by its key; none where it was not
    /// read.
    #[serde(flatten)]
    pub values: BTreeMap<String, Vec<Option<f64>>>,
}

impl LegSamples {
    /// The metric under `key` every `step_ms` up to `run_ms`: at each moment
    /// the sample whose interval holds it; not a number past the last
    /// sample, or where the sample has no value.
    pub fn every(&self, key: &str, run_ms: f64, step_ms: f64) -> Vec<f64> {
        let Some(values) = self.values.get(key) else {
            return Vec::new();
        };
        let mut sample = 0;
        (0..=(run_ms / step_ms) as usize)
            .map(|step| {
                let now = step as f64 * step_ms;
                while sample < self.t_ms.len() && self.t_ms[sample] < now {
                    sample += 1;
                }
                values.get(sample).copied().flatten().unwrap_or(f64::NAN)
            })
            .collect()
    }
}

impl PerfRunFrames {
    pub fn parse(json: &str) -> Result<Self, String> {
        serde_json::from_str(json)
            .map_err(|error| format!("The run's frames did not parse: {error}"))
    }
}

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

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PerfSubject {
    pub name: String,
    /// The framework and the version measured, as `egui 0.36.2`.
    pub label: String,
    /// The file the app draws the gauntlet in, from the repository's root.
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PerfScenario {
    pub legs: usize,
    /// The results of each start, by its name in [`STARTS`].
    #[serde(default)]
    pub starts: BTreeMap<String, PerfStart>,
}

/// One start's results in a scenario.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct PerfStart {
    /// Each subject's medians over its legs of the start.
    pub summary: BTreeMap<String, BTreeMap<String, f64>>,
    /// Between two builds, each deciding metric's verdict on the second.
    #[serde(default)]
    pub verdicts: BTreeMap<String, String>,
    /// Why a subject has no legs of the start: its first failure there.
    #[serde(default)]
    pub failures: BTreeMap<String, String>,
    /// The metrics a second run judged worse again.
    #[serde(default)]
    pub confirmed: Vec<String>,
}

/// The starts a run measures of every subject in each round, each with its
/// title: its first start after its install, its data cleared as the install
/// leaves it, and its second start.
pub const STARTS: [(&str, &str); 2] = [("first", "First start"), ("second", "Second start")];

impl PerfRun {
    /// The day the run started, as its start time writes it.
    fn date(&self) -> &str {
        self.started_at.get(..10).unwrap_or(&self.started_at)
    }

    fn start(&self, scenario: &str, start: &str) -> Option<&PerfStart> {
        self.scenarios.get(scenario)?.starts.get(start)
    }

    fn median(&self, scenario: &str, start: &str, subject: usize, metric: &str) -> Option<f64> {
        let name = &self.subjects.get(subject)?.name;
        self.start(scenario, start)?
            .summary
            .get(name)?
            .get(metric)
            .copied()
    }

    /// Whether the run kept each subject's first and second starts apart, as
    /// every chart shows them.
    fn keeps_starts(&self) -> bool {
        STARTS
            .iter()
            .all(|(start, _)| self.protocol.starts.iter().any(|kept| kept == start))
    }

    /// Whether the run compares frameworks: Compose is among its subjects,
    /// or it is the browser run, whose Compose may have failed to start.
    fn compares_frameworks(&self) -> bool {
        self.kind == "browser"
            || self
                .subjects
                .iter()
                .any(|subject| subject.name == "compose")
    }
}

impl PerfIndex {
    /// Parses `index.json`.
    pub fn parse(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|error| format!("The run index did not parse: {error}"))
    }

    fn nightly(&self) -> Vec<&PerfRun> {
        self.runs
            .iter()
            .filter(|run| run.kind == "nightly" && run.keeps_starts())
            .collect()
    }

    /// The latest framework comparison on each device, the newest first.
    fn latest_framework_runs(&self) -> Vec<&PerfRun> {
        let mut devices = Vec::new();
        self.runs
            .iter()
            .rev()
            .filter(|run| {
                let first = run.compares_frameworks()
                    && run.keeps_starts()
                    && !devices.contains(&run.device.as_str());
                if first {
                    devices.push(run.device.as_str());
                }
                first
            })
            .collect()
    }
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
    cranpose_core::LaunchedEffect(refresh.get(), move |scope| {
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
                             highest. Bars: medians over the run, best first."
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
    let (top, step) = scale(f64::from(peak));
    Plot(palette, top, step, move |scope| {
        let size = scope.size();
        let count = main.len();
        let at = |index: usize, value: f32| Point {
            x: if count > 1 {
                TREND_INSET + index as f32 * (size.width - 2.0 * TREND_INSET) / (count - 1) as f32
            } else {
                size.width / 2.0
            },
            y: chart_y(size.height, f64::from(value), top),
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
    cranpose_core::LaunchedEffect(file.clone(), move |scope| {
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

/// One subject's line on a [`LaunchChart`].
#[derive(Clone, Debug, PartialEq)]
struct LaunchLine {
    name: String,
    label: String,
    color: Color,
    /// The frame rate over the whole run, the median of the subject's legs.
    fps: Option<f64>,
    emphasized: bool,
    /// Why the subject has no legs, when a launch failed.
    failure: Option<String>,
}

/// Colours for the subjects' lines, in turn; Cranpose's take the accent.
const LINE_COLORS: [(u8, u8, u8); 10] = [
    (0xE6, 0x9F, 0x00),
    (0x00, 0x9E, 0x73),
    (0xCC, 0x79, 0xA7),
    (0xD5, 0x5E, 0x00),
    (0x56, 0xB4, 0xE9),
    (0x8B, 0x5C, 0xF6),
    (0x7C, 0x8A, 0x3C),
    (0xEF, 0x44, 0x44),
    (0x0E, 0x74, 0x90),
    (0xA1, 0x62, 0x07),
];

/// Every subject's line for `run`'s `scenario`, in the order of `subjects`:
/// main's Cranpose in the accent, the release in grey, the others in turn.
fn launch_lines(
    palette: Palette,
    run: &PerfRun,
    scenario: &str,
    start: &str,
    subjects: &[(usize, PerfSubject)],
) -> Vec<LaunchLine> {
    let mut others = 0;
    subjects
        .iter()
        .map(|(index, subject)| {
            let color = match subject.name.as_str() {
                "cranpose" => palette.accent,
                "cranpose-release" => palette.baseline,
                _ => {
                    let (red, green, blue) = LINE_COLORS[others % LINE_COLORS.len()];
                    others += 1;
                    Color::from_rgb_u8(red, green, blue)
                }
            };
            LaunchLine {
                name: subject.name.clone(),
                label: subject.label.clone(),
                color,
                fps: run.median(scenario, start, *index, "fps"),
                emphasized: subject.name == "cranpose",
                failure: run
                    .start(scenario, start)
                    .and_then(|entry| entry.failures.get(&subject.name).cloned()),
            }
        })
        .collect()
}

/// One subject's frame rate through the run, across its launches.
#[derive(Clone, Debug, PartialEq)]
struct LaunchCurve {
    line: LaunchLine,
    median: Vec<f64>,
    lowest: Vec<f64>,
    highest: Vec<f64>,
    first_frame_ms: Option<f64>,
}

/// Each line's `curve` from the launch, every [`CHART_STEP_MS`] up to
/// `run_ms`, over its legs of `start` no other process disturbed: the frame
/// rate, or a metric the run sampled.
fn launch_curves(
    frames: &PerfRunFrames,
    scenario: &str,
    start: &str,
    run_ms: f64,
    lines: &[LaunchLine],
    curve: Curve,
) -> Vec<LaunchCurve> {
    let Some(entry) = frames
        .scenarios
        .iter()
        .find(|entry| entry.scenario == scenario)
    else {
        return Vec::new();
    };
    lines
        .iter()
        .filter_map(|line| {
            let legs = entry
                .legs
                .iter()
                .filter(|leg| leg.subject == line.name && leg.start == start && !leg.disturbed);
            let traces: Vec<Vec<f64>> = legs
                .clone()
                .filter_map(|leg| leg_trace(leg, curve, run_ms))
                .collect();
            if traces.is_empty() {
                return None;
            }
            let mut firsts: Vec<f64> = legs
                .filter_map(|leg| leg.frame_ms.first().copied())
                .collect();
            firsts.sort_by(f64::total_cmp);
            let [median, lowest, highest] = across(&traces);
            Some(LaunchCurve {
                line: line.clone(),
                median,
                lowest,
                highest,
                first_frame_ms: (!firsts.is_empty()).then(|| middle(&firsts)),
            })
        })
        .collect()
}

/// A launch's `curve` every [`CHART_STEP_MS`] up to `run_ms`, when it read
/// any.
fn leg_trace(leg: &PerfFramesLeg, curve: Curve, run_ms: f64) -> Option<Vec<f64>> {
    match curve {
        Curve::Frames if !leg.frame_ms.is_empty() => {
            Some(launch_fps(&leg.frame_ms, run_ms, CHART_STEP_MS))
        }
        Curve::Samples(key) => Some(leg.samples.every(key, run_ms, CHART_STEP_MS))
            .filter(|trace| trace.iter().any(|value| value.is_finite())),
        Curve::Frames | Curve::None(_) => None,
    }
}

/// The median, the lowest and the highest of `traces` at each step, leaving
/// out the launches that read nothing there; not a number where none did.
fn across(traces: &[Vec<f64>]) -> [Vec<f64>; 3] {
    let steps = traces.iter().map(Vec::len).min().unwrap_or(0);
    let mut read: [Vec<f64>; 3] = std::array::from_fn(|_| Vec::with_capacity(steps));
    let mut values = Vec::with_capacity(traces.len());
    for step in 0..steps {
        values.clear();
        values.extend(
            traces
                .iter()
                .map(|trace| trace[step])
                .filter(|value| value.is_finite()),
        );
        values.sort_by(f64::total_cmp);
        let picked = match (values.first(), values.last()) {
            (Some(lowest), Some(highest)) => [middle(&values), *lowest, *highest],
            _ => [f64::NAN; 3],
        };
        for (series, value) in read.iter_mut().zip(picked) {
            series.push(value);
        }
    }
    read
}

/// The median of sorted `values`.
fn middle(values: &[f64]) -> f64 {
    match values.len() {
        0 => 0.0,
        length if length % 2 == 1 => values[length / 2],
        length => (values[length / 2 - 1] + values[length / 2]) / 2.0,
    }
}

/// What a chart says in place of its curves while its run's frames are not
/// at hand, and whether that reports a failure.
fn frames_note(frames: &FramesState) -> Option<(String, bool)> {
    match frames {
        FramesState::Loaded(_) => None,
        FramesState::Loading => Some(("Loading the run's frames…".to_string(), false)),
        FramesState::Failed(error) => Some((error.clone(), true)),
    }
}

/// A path through `values`, one every chart step from the launch, broken
/// where no launch read a value.
fn polyline(
    path: &mut Path,
    values: impl Iterator<Item = (usize, f64)>,
    at: impl Fn(usize, f64) -> Point,
) {
    let mut drawing = false;
    for (step, value) in values {
        if !value.is_finite() {
            drawing = false;
        } else if drawing {
            path.line_to(at(step, value));
        } else {
            path.move_to(at(step, value));
            drawing = true;
        }
    }
}

/// The stretches of steps over which `values` reads a value.
fn read_stretches(values: &[f64]) -> impl Iterator<Item = std::ops::Range<usize>> + '_ {
    let mut step = 0;
    std::iter::from_fn(move || {
        while step < values.len() && !values[step].is_finite() {
            step += 1;
        }
        let start = step;
        while step < values.len() && values[step].is_finite() {
            step += 1;
        }
        (start < step).then_some(start..step)
    })
}

/// Each curve: its band from the lowest to the highest launch, then its
/// median line, each value capped at the top.
fn draw_launch_curves(scope: &mut dyn DrawScope, curves: &[LaunchCurve], run_ms: f64, top: f64) {
    let size = scope.size();
    let at = |step: usize, value: f64| Point {
        x: size.width * (step as f64 * CHART_STEP_MS / run_ms) as f32,
        y: chart_y(size.height, value, top),
    };
    for curve in curves {
        let color = curve.line.color;
        let mut band = Path::new();
        for stretch in read_stretches(&curve.highest) {
            polyline(
                &mut band,
                stretch.clone().map(|step| (step, curve.highest[step])),
                at,
            );
            for step in stretch.rev() {
                band.line_to(at(step, curve.lowest[step]));
            }
            band.close();
        }
        scope.draw_path(&band, Brush::solid(color.with_alpha(0.10)), DrawStyle::Fill);
    }
    // Cranpose's line last, over the others.
    for curve in curves
        .iter()
        .filter(|curve| !curve.line.emphasized)
        .chain(curves.iter().filter(|curve| curve.line.emphasized))
    {
        let mut line = Path::new();
        polyline(&mut line, curve.median.iter().copied().enumerate(), at);
        let width = if curve.line.emphasized { 2.5 } else { 1.5 };
        scope.draw_path(
            &line,
            Brush::solid(curve.line.color),
            DrawStyle::Stroke(Stroke::new(width)),
        );
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
    ScenarioChips(palette, run.scenarios.keys().cloned().collect(), scenario);
    let frames = run_frames(source, run.file.clone()).get();
    let scenario = shown_scenario(&run, &scenario.get());
    let lines = launch_lines(palette, &run, &scenario, start, &subjects);
    LaunchChart(
        palette,
        frames,
        run_ms(&run),
        scenario.clone(),
        start,
        lines.clone(),
        metric,
    );
    SubjectBars(palette, run, subjects, scenario, start, metric, lines);
}

/// How tall a launch chart is, in points.
const CHART_HEIGHT: f32 = 190.0;
/// The room over the top grid line, as a share of its value: a line at the
/// top stays off the chart's edge.
const CHART_HEADROOM: f64 = 1.08;

/// `metric` against the milliseconds since the launch, for every subject:
/// each line is the median of its launches, over a band from the lowest to
/// the highest. The frame rate's grid is every 30 fps; a sampled metric's
/// fits its values.
#[composable]
fn LaunchChart(
    palette: Palette,
    frames: FramesState,
    run_ms: f64,
    scenario: String,
    start: &'static str,
    lines: Vec<LaunchLine>,
    metric: Metric,
) {
    let curve = metric.curve();
    if let Curve::None(why) = curve {
        Text(
            why,
            Modifier::empty(),
            text_style(12.0, palette.muted, None),
        );
        return;
    }
    if let Some((note, failed)) = frames_note(&frames) {
        let color = if failed { palette.worse } else { palette.muted };
        Text(note, Modifier::empty(), text_style(12.0, color, None));
        return;
    }
    let curves = match &frames {
        FramesState::Loaded(frames) => {
            launch_curves(frames, &scenario, start, run_ms, &lines, curve)
        }
        _ => Vec::new(),
    };
    if curves.is_empty() {
        Text(
            if curve == Curve::Frames {
                "No launch of this start kept its frames."
            } else {
                "No launch of this start kept samples of it: runs before the sampler have none."
            },
            Modifier::empty(),
            text_style(12.0, palette.muted, None),
        );
        return;
    }
    let (top, step) = if curve == Curve::Frames {
        (chart_top(&curves), FPS_STEP)
    } else {
        scale(
            curves
                .iter()
                .flat_map(|curve| curve.highest.iter().copied())
                .filter(|value| value.is_finite())
                .fold(0.0f64, f64::max),
        )
    };
    Plot(palette, top, step, move |scope| {
        draw_launch_curves(scope, &curves, run_ms, top);
    });
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpaceBetween),
        move || {
            for quarter in 0..=4 {
                let ms = run_ms * f64::from(quarter) / 4.0;
                Text(
                    if quarter == 4 {
                        format!("{ms:.0} ms")
                    } else {
                        format!("{ms:.0}")
                    },
                    Modifier::empty(),
                    text_style(11.0, palette.muted, None),
                );
            }
        },
    );
}

/// The frame rate a launch chart's top stands for: the next `FPS_STEP` above
/// its highest median, two steps at least. A median a frame or two over a
/// grid line, as a window can count at 60 Hz, stays under it.
fn chart_top(curves: &[LaunchCurve]) -> f64 {
    let peak = curves
        .iter()
        .flat_map(|curve| curve.median.iter().copied())
        .fold(0.0f64, f64::max);
    (FPS_STEP * (peak / FPS_STEP - 0.1).ceil()).max(2.0 * FPS_STEP)
}

/// The frame rates between a launch chart's grid lines.
const FPS_STEP: f64 = 30.0;

/// A chart's top and the step between its grid lines for values up to
/// `peak`: the step 1, 2 or 5 times a power of ten that splits the top in
/// three at most.
fn scale(peak: f64) -> (f64, f64) {
    let peak = if peak.is_finite() { peak.max(1.0) } else { 1.0 };
    let rough = peak / 3.0;
    let power = 10f64.powf(rough.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|multiple| multiple * power)
        .find(|step| *step >= rough)
        .unwrap_or(10.0 * power);
    (step * (peak / step).ceil(), step)
}

/// The grid lines from 0 to `top`, every `step`.
fn grid(top: f64, step: f64) -> impl Iterator<Item = f64> {
    (0..)
        .map(move |line| f64::from(line) * step)
        .take_while(move |value| *value <= top)
}

/// Where `value` sits on a chart `height` tall whose top grid line stands
/// for `top`; a value over the top sits on it.
fn chart_y(height: f32, value: f64, top: f64) -> f32 {
    height * (1.0 - (value.min(top) / (top * CHART_HEADROOM)) as f32)
}

/// What a chart paints over its grid.
type ChartDraw = Rc<dyn Fn(&mut dyn DrawScope)>;

/// A chart's area: a labelled grid line every `step` from 0 to `top`, under
/// what `draw` paints.
#[composable]
fn Plot(palette: Palette, top: f64, step: f64, draw: impl Fn(&mut dyn DrawScope) + 'static) {
    let draw: ChartDraw = Rc::new(draw);
    Box(
        Modifier::empty().fill_max_width().height(CHART_HEIGHT),
        BoxSpec::default(),
        move || {
            let draw = Rc::clone(&draw);
            Canvas(Modifier::empty().fill_max_size(), move |scope| {
                let size = scope.size();
                for value in grid(top, step) {
                    let y = chart_y(size.height, value, top).min(size.height - 0.5);
                    scope.draw_line(
                        Brush::solid(palette.baseline.with_alpha(0.3)),
                        Point { x: 0.0, y },
                        Point { x: size.width, y },
                        Stroke::new(1.0),
                    );
                }
                draw(scope);
            });
            for value in grid(top, step) {
                let y = chart_y(CHART_HEIGHT, value, top) - 14.0;
                Text(
                    if step < 1.0 {
                        format!("{value:.1}")
                    } else {
                        format!("{value:.0}")
                    },
                    Modifier::empty().offset(2.0, y.max(0.0)),
                    text_style(10.0, palette.muted, None),
                );
            }
        },
    );
}

/// Every subject's `metric` for `scenario` and `start` as a bar in its line's
/// colour, best first, alphabetical among equals: its dot, its name, which
/// opens its source at the commit the run measured, and the bar with its
/// value, or why it has none.
#[composable]
fn SubjectBars(
    palette: Palette,
    run: Rc<PerfRun>,
    subjects: Rc<Vec<(usize, PerfSubject)>>,
    scenario: String,
    start: &'static str,
    metric: Metric,
    lines: Vec<LaunchLine>,
) {
    let commit = run.main.clone().unwrap_or_else(|| "main".to_string());
    let values: Vec<Option<f64>> = subjects
        .iter()
        .map(|(index, _)| run.median(&scenario, start, *index, metric.key()))
        .collect();
    let full = metric.full_scale(&values);
    let mut order: Vec<usize> = (0..subjects.len()).collect();
    order.sort_by(|&a, &b| metric.best_first(values[a], values[b]));
    for position in order {
        let (index, subject) = &subjects[position];
        let line = lines.iter().find(|line| line.name == subject.name);
        // A subject with legs but without this metric gets a dash; one with
        // no legs says why.
        let measured = run.median(&scenario, start, *index, "fps").is_some();
        let (share, value) = match values[position] {
            Some(value) => (
                Some(if full > 0.0 {
                    (value / full) as f32
                } else {
                    0.0
                }),
                metric.format(value),
            ),
            None if measured => (None, "–".to_string()),
            None => (
                None,
                line.and_then(|line| line.failure.clone())
                    .unwrap_or_else(|| "no frames".to_string()),
            ),
        };
        SubjectBar(
            palette,
            subject.label.clone(),
            subject
                .source
                .as_ref()
                .map(|source| format!("{SOURCE_URL}/{commit}/{source}")),
            line.map_or(palette.baseline, |line| line.color),
            line.is_some_and(|line| line.emphasized),
            share,
            value,
        );
    }
}

/// One subject's row: the dot of its line's colour, its name, and its value
/// on a bar `share` of the full length that fades into its colour, or, with
/// no share, why it has no value.
#[composable]
fn SubjectBar(
    palette: Palette,
    label: String,
    url: Option<String>,
    color: Color,
    emphasized: bool,
    share: Option<f32>,
    value: String,
) {
    let uri_handler = local_uri_handler().current();
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Dot(color);
            let name = Modifier::empty().weight(0.44);
            let style = text_style(12.0, palette.text, emphasized.then_some(FontWeight::BOLD));
            match url.clone() {
                Some(url) => {
                    let uri_handler = Rc::clone(&uri_handler);
                    Text(
                        label.clone(),
                        name.clickable(move |_| {
                            if let Err(error) = uri_handler.open_uri(&url) {
                                log::error!("Could not open {url}: {error:#}");
                            }
                        }),
                        style,
                    );
                }
                None => {
                    Text(label.clone(), name, style);
                }
            }
            let value = value.clone();
            Box(
                Modifier::empty().weight(0.56).height(22.0),
                BoxSpec::new().content_alignment(Alignment::CENTER_START),
                move || {
                    let Some(share) = share else {
                        TextWithOptions(
                            value.clone(),
                            Modifier::empty().fill_max_width().padding_horizontal(4.0),
                            text_style(11.0, palette.muted, None),
                            TextOptions {
                                overflow: TextOverflow::Ellipsis,
                                max_lines: Some(1),
                                ..TextOptions::default()
                            },
                        );
                        return;
                    };
                    Canvas(Modifier::empty().fill_max_size(), move |scope| {
                        let size = scope.size();
                        let radius = cranpose_ui::CornerRadii::uniform(size.height / 2.0);
                        scope.draw_round_rect_at(
                            cranpose_ui::Rect {
                                x: 0.0,
                                y: 0.0,
                                width: size.width,
                                height: size.height,
                            },
                            Brush::solid(palette.background),
                            radius,
                        );
                        let width = (size.width * share.clamp(0.0, 1.0)).max(size.height);
                        scope.draw_round_rect_at(
                            cranpose_ui::Rect {
                                x: 0.0,
                                y: 0.0,
                                width,
                                height: size.height,
                            },
                            Brush::horizontal_gradient(
                                vec![color.with_alpha(0.35), color],
                                0.0,
                                width,
                            ),
                            radius,
                        );
                    });
                    Text(
                        value.clone(),
                        Modifier::empty().padding_horizontal(9.0),
                        text_style(11.5, palette.text, Some(FontWeight::BOLD)),
                    );
                },
            );
        },
    );
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
