//! The Performance tab: the nightly comparison of the latest release against
//! main, and Cranpose against other frameworks, read from the `perf-data`
//! branch that `scripts/perf/publish.py` writes.

use std::{cmp::Ordering, collections::BTreeMap, rc::Rc};

use cranpose_core::{self, MutableState};
use cranpose_services::{isSystemInDarkTheme, local_http_client, local_uri_handler, HttpClientRef};
use cranpose_ui::{
    composable, Alignment, Box, BoxSpec, Brush, Button, ButtonSpec, Canvas, Color, Column,
    ColumnSpec, DrawStyle, LinearArrangement, Modifier, Path, Point, Row, RowSpec, ScrollState,
    Stroke, Text, VerticalAlignment,
};
use serde::Deserialize;

use super::demo_text::text_style;

/// The run index the nightly publishes, served as raw JSON from the branch.
const INDEX_URL: &str =
    "https://raw.githubusercontent.com/samoylenkodmitry/Cranpose/perf-data/index.json";
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
    pub confirmed_regressions: BTreeMap<String, Vec<String>>,
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
    pub summary: BTreeMap<String, BTreeMap<String, f64>>,
    pub verdicts: BTreeMap<String, String>,
}

impl PerfRun {
    fn median(&self, scenario: &str, subject: usize, metric: &str) -> Option<f64> {
        let name = &self.subjects.get(subject)?.name;
        self.scenarios
            .get(scenario)?
            .summary
            .get(name)?
            .get(metric)
            .copied()
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
            .filter(|run| run.kind == "nightly")
            .collect()
    }

    /// The latest framework comparison on each device, the newest first.
    fn latest_framework_runs(&self) -> Vec<&PerfRun> {
        let mut devices = Vec::new();
        self.runs
            .iter()
            .rev()
            .filter(|run| {
                let first = run.compares_frameworks() && !devices.contains(&run.device.as_str());
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

/// What the trend and the framework bars show of each run's medians.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Metric {
    Fps,
    CpuPerFrame,
    Ram,
    GpuRam,
    CpuClock,
    GpuClock,
}

impl Metric {
    const ALL: [Self; 6] = [
        Self::Fps,
        Self::CpuPerFrame,
        Self::Ram,
        Self::GpuRam,
        Self::CpuClock,
        Self::GpuClock,
    ];

    /// The median's name in the run index.
    fn key(self) -> &'static str {
        match self {
            Self::Fps => "fps",
            Self::CpuPerFrame => "cpu_ms_per_frame",
            Self::Ram => "ram_mb",
            Self::GpuRam => "gpu_ram_mb",
            Self::CpuClock => "cpu_mhz",
            Self::GpuClock => "gpu_mhz",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Fps => "fps",
            Self::CpuPerFrame => "CPU ms per frame",
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
                "Frames the display showed each second, counted outside the app; 60 at most."
            }
            Self::CpuPerFrame => {
                "CPU time of the app and every process it started, per frame shown."
            }
            Self::Ram => {
                "Memory the app holds at the window's end: PSS on Android, the footprint of the \
                 app and every process it started on macOS."
            }
            Self::GpuRam => {
                "The GPU's part of that memory: GL and EGL mtrack on Android; Metal buffers, \
                 textures and window surfaces on macOS."
            }
            Self::CpuClock => {
                "Mean clock over the window of the big cores on Android, the performance cores \
                 on macOS."
            }
            Self::GpuClock => "Mean GPU clock over the window.",
        }
    }

    fn format(self, value: f64) -> String {
        match self {
            Self::Fps | Self::CpuPerFrame => format!("{value:.1}"),
            Self::Ram | Self::GpuRam | Self::CpuClock | Self::GpuClock => format!("{value:.0}"),
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
        let client = client.clone();
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
    match state.get() {
        LoadState::Loaded(index) => PerformanceDashboard(index, Some(refresh)),
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
                text_style(15.0, palette.muted, false),
            );
        },
    );
}

/// Everything the tab shows for one run index. `refresh` adds a Refresh
/// button that bumps it.
#[composable]
pub fn PerformanceDashboard(index: Rc<PerfIndex>, refresh: Option<MutableState<u64>>) {
    let palette = Palette::new(isSystemInDarkTheme());
    let scroll = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
    let selected = cranpose_core::rememberMutableStateOf(|| "gauntlet".to_string());
    let metric = cranpose_core::rememberMutableStateOf(|| Metric::Fps);
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(palette.background)
            .vertical_scroll(scroll, false)
            .padding(20.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(16.0)),
        move || {
            Header(palette, refresh);
            let nightly: Vec<PerfRun> = index.nightly().into_iter().cloned().collect();
            match nightly.last() {
                Some(latest) => {
                    LatestNightly(palette, latest.clone());
                    Trend(palette, Rc::new(nightly.clone()), selected, metric);
                }
                None => {
                    Text(
                        "No nightly run has been published yet.",
                        Modifier::empty(),
                        text_style(14.0, palette.muted, false),
                    );
                }
            }
            for run in index.latest_framework_runs() {
                Frameworks(palette, run.clone());
            }
        },
    );
}

#[composable]
fn Header(palette: Palette, refresh: Option<MutableState<u64>>) {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Column(
                Modifier::empty().weight(1.0),
                ColumnSpec::default(),
                move || {
                    Text(
                        "Performance",
                        Modifier::empty(),
                        text_style(24.0, palette.text, true),
                    );
                    Text(
                    "Each night the latest release and main draw the same scenarios on the same \
                     phone, measured from outside the app.",
                    Modifier::empty(),
                    text_style(13.0, palette.muted, false),
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
                            text_style(13.0, Color::WHITE, true),
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
            .rounded_corners(12.0)
            .padding(16.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                title.clone(),
                Modifier::empty(),
                text_style(16.0, palette.text, true),
            );
            content();
        },
    );
}

#[composable]
fn LatestNightly(palette: Palette, run: PerfRun) {
    let release = run.release.clone().unwrap_or_default();
    let main = run.main.clone().unwrap_or_default();
    let title = format!(
        "{release} against main {} on {}, {}",
        &main[..main.len().min(9)],
        run.device,
        &run.started_at[..run.started_at.len().min(10)]
    );
    Card(palette, title, move || {
        let headers = [
            "scenario",
            "release",
            "main",
            "change",
            "CPU ms/frame",
            "to screen ms",
        ];
        TableRow(
            headers.map(str::to_string).to_vec(),
            vec![palette.muted; 6],
            true,
        );
        for (scenario, entry) in &run.scenarios {
            let fps = (
                run.median(scenario, 0, "fps"),
                run.median(scenario, 1, "fps"),
            );
            let change = match fps {
                (Some(before), Some(after)) if before > 0.0 => {
                    format!("{:+.1}%", (after - before) / before * 100.0)
                }
                _ => "–".to_string(),
            };
            let pair = |metric: &str| match (
                run.median(scenario, 0, metric),
                run.median(scenario, 1, metric),
            ) {
                (Some(before), Some(after)) => format!("{before:.1} → {after:.1}"),
                _ => "–".to_string(),
            };
            let regressed = run.confirmed_regressions.contains_key(scenario);
            let cells = vec![
                if regressed {
                    format!("{scenario} ⚠")
                } else {
                    scenario.clone()
                },
                figure(fps.0),
                figure(fps.1),
                change,
                pair("cpu_ms_per_frame"),
                pair("desired_to_present_p50_ms"),
            ];
            let colors = vec![
                if regressed {
                    palette.worse
                } else {
                    palette.text
                },
                palette.muted,
                palette.verdict(entry.verdicts.get("fps")),
                palette.verdict(entry.verdicts.get("fps")),
                palette.verdict(entry.verdicts.get("cpu_ms_per_frame")),
                palette.verdict(entry.verdicts.get("desired_to_present_p50_ms")),
            ];
            TableRow(cells, colors, false);
        }
        Text(
            "Green: main is better; red: worse; amber: not settled within four pairs of legs. \
             ⚠ marks a regression a second run confirmed.",
            Modifier::empty(),
            text_style(12.0, palette.muted, false),
        );
    });
}

#[composable]
fn TableRow(cells: Vec<String>, colors: Vec<Color>, header: bool) {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            for (index, (cell, color)) in cells.iter().zip(&colors).enumerate() {
                Text(
                    cell.clone(),
                    Modifier::empty().weight(if index == 0 { 1.6 } else { 1.0 }),
                    text_style(13.0, *color, header || index == 0),
                );
            }
        },
    );
}

#[composable]
fn Trend(
    palette: Palette,
    nightly: Rc<Vec<PerfRun>>,
    selected: MutableState<String>,
    metric: MutableState<Metric>,
) {
    let scenario = selected.get();
    let title = format!("{scenario}: {}, night by night", metric.get().name());
    Card(palette, title, move || {
        let names: Vec<String> = nightly
            .last()
            .map(|run| run.scenarios.keys().cloned().collect())
            .unwrap_or_default();
        let chosen = names.iter().position(|name| *name == selected.get());
        let choices = names.clone();
        Chips(palette, names, chosen, move |index| {
            selected.set(choices[index].clone());
        });
        MetricChips(palette, metric);
        let scenario = selected.get();
        let shown = metric.get();
        let series = |subject: usize| -> Vec<f32> {
            nightly
                .iter()
                .map(|run| {
                    run.median(&scenario, subject, shown.key())
                        .unwrap_or(f64::NAN) as f32
                })
                .collect()
        };
        let regressed: Vec<bool> = nightly
            .iter()
            .map(|run| run.confirmed_regressions.contains_key(&scenario))
            .collect();
        LineChart(palette, series(0), series(1), regressed);
        Text(
            format!(
                "Grey: the release; blue: main; red dots: confirmed regressions. {}",
                shown.about()
            ),
            Modifier::empty(),
            text_style(12.0, palette.muted, false),
        );
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

/// A row of choices, the `chosen` one highlighted.
#[composable]
fn Chips(
    palette: Palette,
    names: Vec<String>,
    chosen: Option<usize>,
    choose: impl Fn(usize) + 'static,
) {
    let choose: Rc<dyn Fn(usize)> = Rc::new(choose);
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(6.0)),
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
                        .rounded_corners(12.0)
                        .padding_symmetric(10.0, 4.0),
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
                                    false,
                                ),
                            );
                        }
                    },
                );
            }
        },
    );
}

#[composable]
fn LineChart(palette: Palette, release: Vec<f32>, main: Vec<f32>, regressed: Vec<bool>) {
    Canvas(
        Modifier::empty().fill_max_width().height(160.0),
        move |scope| {
            let size = scope.size();
            let finite = release
                .iter()
                .chain(&main)
                .copied()
                .filter(|value| value.is_finite());
            let top = finite.fold(0.0f32, f32::max).max(1.0) * 1.1;
            let step = if main.len() > 1 {
                size.width / (main.len() - 1) as f32
            } else {
                0.0
            };
            let at = |index: usize, value: f32| Point {
                x: index as f32 * step,
                y: size.height * (1.0 - value / top),
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
                    scope.draw_circle(Brush::solid(series.1), at(index, *value), 3.0);
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
        },
    );
}

#[composable]
fn Frameworks(palette: Palette, run: PerfRun) {
    // Best first for the metric shown, alphabetical among equals, each with
    // the version measured and a link to its source at the commit the run
    // measured. Cranpose's bars carry the accent.
    let mut subjects: Vec<(usize, PerfSubject)> =
        run.subjects.iter().cloned().enumerate().collect();
    subjects.sort_by(|(_, a), (_, b)| a.name.cmp(&b.name));
    let commit = run.main.clone().unwrap_or_else(|| "main".to_string());
    let metric = cranpose_core::rememberMutableStateOf(|| Metric::Fps);
    let place = if run.kind == "browser" {
        "Frameworks in the browser,"
    } else {
        "Frameworks on the"
    };
    let title = format!(
        "{place} {}, {}",
        run.device,
        &run.started_at[..run.started_at.len().min(10)]
    );
    Card(palette, title, move || {
        MetricChips(palette, metric);
        let shown = metric.get();
        for scenario in run.scenarios.keys() {
            Text(
                scenario.clone(),
                Modifier::empty(),
                text_style(13.0, palette.text, true),
            );
            let values: Vec<Option<f64>> = subjects
                .iter()
                .map(|(index, _)| run.median(scenario, *index, shown.key()))
                .collect();
            let full = shown.full_scale(&values);
            let mut order: Vec<usize> = (0..subjects.len()).collect();
            order.sort_by(|&a, &b| shown.best_first(values[a], values[b]));
            for position in order {
                let (_, subject) = &subjects[position];
                let value = &values[position];
                let color = if subject.name.starts_with("cranpose") {
                    palette.accent
                } else {
                    palette.baseline
                };
                let url = subject
                    .source
                    .as_ref()
                    .map(|source| format!("{SOURCE_URL}/{commit}/{source}"));
                let share = match value {
                    Some(value) if full > 0.0 => (value / full) as f32,
                    _ => 0.0,
                };
                let text = value.map_or_else(|| "–".to_string(), |value| shown.format(value));
                MetricBar(palette, subject.label.clone(), url, share, text, color);
            }
        }
        Text(
            shown.about(),
            Modifier::empty(),
            text_style(12.0, palette.muted, false),
        );
    });
}

/// A framework, which opens its source when it has one, and its value as a
/// bar `share` of the full length with the number on it.
#[composable]
fn MetricBar(
    palette: Palette,
    label: String,
    url: Option<String>,
    share: f32,
    value: String,
    color: Color,
) {
    let uri_handler = local_uri_handler().current();
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let name = Modifier::empty().width(220.0);
            match url.clone() {
                Some(url) => {
                    let uri_handler = uri_handler.clone();
                    Text(
                        label.clone(),
                        name.clickable(move |_| {
                            if let Err(error) = uri_handler.open_uri(&url) {
                                log::error!("Could not open {url}: {error:#}");
                            }
                        }),
                        text_style(12.0, palette.accent, false),
                    );
                }
                None => {
                    Text(label.clone(), name, text_style(12.0, palette.muted, false));
                }
            }
            let value = value.clone();
            Box(
                Modifier::empty().weight(1.0).height(18.0),
                BoxSpec::new().content_alignment(Alignment::CENTER_START),
                move || {
                    Canvas(Modifier::empty().fill_max_size(), move |scope| {
                        let size = scope.size();
                        scope.draw_round_rect_at(
                            cranpose_ui::Rect {
                                x: 0.0,
                                y: 0.0,
                                width: size.width,
                                height: size.height,
                            },
                            Brush::solid(palette.background),
                            cranpose_ui::CornerRadii::uniform(9.0),
                        );
                        scope.draw_round_rect_at(
                            cranpose_ui::Rect {
                                x: 0.0,
                                y: 0.0,
                                width: size.width * share.clamp(0.0, 1.0),
                                height: size.height,
                            },
                            Brush::solid(color),
                            cranpose_ui::CornerRadii::uniform(9.0),
                        );
                    });
                    Text(
                        value.clone(),
                        Modifier::empty().padding_horizontal(8.0),
                        text_style(11.0, palette.text, true),
                    );
                },
            );
        },
    );
}
