// The launch chart of the Performance tab: every subject's metric since the
// launch, each as the median of its launches over a band.

use cranpose_ui::{
    composable, Brush, Color, DrawScope, DrawStyle, LinearArrangement, Modifier, Path, Point, Row,
    RowSpec, Stroke, Text,
};

use super::{
    launch_fps,
    plot::{chart_y, scale, Plot},
    text_style, Curve, FramesState, Metric, Palette, PerfFramesLeg, PerfRun, PerfRunFrames,
    PerfSubject, CHART_STEP_MS,
};

/// One subject's line on a [`LaunchChart`].
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LaunchLine {
    pub(super) name: String,
    label: String,
    pub(super) color: Color,
    /// The frame rate over the whole run, the median of the subject's legs.
    fps: Option<f64>,
    pub(super) emphasized: bool,
    /// Why the subject has no legs, when a launch failed.
    pub(super) failure: Option<String>,
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
pub(super) fn launch_lines(
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

/// `metric` against the milliseconds since the launch, for every subject:
/// each line is the median of its launches, over a band from the lowest to
/// the highest. The frame rate's grid is every 30 fps; a sampled metric's
/// fits its values.
#[composable]
pub(super) fn LaunchChart(
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
