// The launch chart of the Performance tab: every subject's metric since the
// launch, each as the median of its launches over a band.

use std::rc::Rc;

use cranpose_ui::{
    composable, text::FontWeight, Button, ButtonSpec, Color, LinearArrangement, Modifier, Row,
    RowSpec, Text, VerticalAlignment,
};

use super::{
    chart_view::{reach, ChartView, Plotted},
    curve_plot::CurvePlot,
    focus::Focus,
    launch_fps, text_style, Curve, FramesState, Palette, PerfFramesLeg, PerfRun, PerfRunFrames,
    PerfSubject, Selection, CHART_STEP_MS,
};

/// One subject's line on a [`LaunchChart`].
#[derive(Clone, Debug, PartialEq)]
pub(super) struct LaunchLine {
    pub(super) subject: usize,
    pub(super) name: String,
    pub(super) label: String,
    pub(super) color: Color,
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
                subject: *index,
                name: subject.name.clone(),
                label: subject.label.clone(),
                color,
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
pub(super) struct LaunchCurve {
    pub(super) line: LaunchLine,
    pub(super) median: Vec<f64>,
    pub(super) lowest: Vec<f64>,
    pub(super) highest: Vec<f64>,
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
            let traces: Vec<Vec<f64>> = entry
                .legs
                .iter()
                .filter(|leg| leg.subject == line.name && leg.start == start && !leg.disturbed)
                .filter_map(|leg| leg_trace(leg, curve, run_ms))
                .collect();
            if traces.is_empty() {
                return None;
            }
            let [median, lowest, highest] = across(&traces);
            Some(LaunchCurve {
                line: line.clone(),
                median,
                lowest,
                highest,
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

/// `metric` against the milliseconds since the launch, for every subject:
/// each line is the median of its launches, over a band from the lowest to
/// the highest. The frame rate's grid is every 30 fps; a sampled metric's
/// fits its values, without the subjects far above its leaders.
#[composable]
pub(super) fn LaunchChart(
    palette: Palette,
    focus: Focus,
    frames: FramesState,
    run_ms: f64,
    selection: Selection,
    lines: Vec<LaunchLine>,
) {
    let metric = selection.metric;
    let curve = metric.curve();
    if let Curve::None(why) = curve {
        Note(palette.muted, why.to_string());
        return;
    }
    if let Some((note, failed)) = frames_note(&frames) {
        Note(if failed { palette.worse } else { palette.muted }, note);
        return;
    }
    let curves = match &frames {
        FramesState::Loaded(frames) => launch_curves(
            frames,
            &selection.scenario,
            selection.start,
            run_ms,
            &lines,
            curve,
        ),
        _ => Vec::new(),
    };
    if curves.is_empty() {
        Note(
            palette.muted,
            if curve == Curve::Frames {
                "No launch of this start kept its frames."
            } else {
                "No launch of this start kept samples of it: runs before the sampler have none."
            }
            .to_string(),
        );
        return;
    }
    ChartBody(
        palette,
        focus,
        Rc::new(Plotted {
            curves,
            run_ms,
            metric,
        }),
    );
}

// A chart of `plotted`'s curves as the bars picked leave them, the time under
// it, and what it says of the rest. It alone follows the picks, so a pick
// does not read the run's frames again.
#[composable]
fn ChartBody(palette: Palette, focus: Focus, plotted: Rc<Plotted>) {
    let view = cranpose_core::rememberKeyed(
        (Rc::as_ptr(&plotted) as usize, focus.picked.get()),
        |(_, picked)| Rc::new(ChartView::new(&plotted, picked)),
    );
    CurvePlot(palette, focus, Rc::clone(&plotted), Rc::clone(&view));
    TimeRuler(palette, plotted.run_ms);
    ChartNote(palette, focus, plotted, view);
}

#[composable]
fn Note(color: Color, text: String) {
    Text(text, Modifier::empty(), text_style(12.0, color, None));
}

#[composable]
fn TimeRuler(palette: Palette, run_ms: f64) {
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

// What a chart says of the subjects it leaves out, or of the bars picked,
// with the way back to every subject.
#[composable]
fn ChartNote(palette: Palette, focus: Focus, plotted: Rc<Plotted>, view: Rc<ChartView>) {
    if !view.picking {
        if let Some(note) = left_out_note(&plotted, &view) {
            Note(palette.muted, note);
        }
        return;
    }
    let picked = focus.picked.get().len();
    let drawn = view.shown.len();
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(10.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text(
                if drawn == 0 {
                    format!("{picked} picked, none with a curve of this metric.")
                } else {
                    format!("{picked} picked: the chart shows only them.")
                },
                Modifier::empty().weight(1.0),
                text_style(12.0, palette.muted, None),
            );
            Button(
                Modifier::empty()
                    .background(palette.accent)
                    .rounded_corners(14.0)
                    .padding_symmetric(12.0, 5.0),
                ButtonSpec::default(),
                move || focus.clear(),
                move || {
                    Text(
                        "Show all",
                        Modifier::empty(),
                        text_style(12.0, Color::WHITE, Some(FontWeight::BOLD)),
                    );
                },
            );
        },
    );
}

// What a chart says of the subjects it leaves out for reaching far above its
// leaders.
fn left_out_note(plotted: &Plotted, view: &ChartView) -> Option<String> {
    if view.left_out.is_empty() {
        return None;
    }
    let metric = plotted.metric;
    let left_out: Vec<&LaunchCurve> = view
        .left_out
        .iter()
        .filter_map(|subject| plotted.curve_of(*subject))
        .collect();
    let far = left_out
        .iter()
        .map(|curve| reach(curve, metric.curve()))
        .fold(0.0, f64::max);
    let names: Vec<&str> = left_out
        .iter()
        .map(|curve| curve.line.label.as_str())
        .collect();
    Some(format!(
        "Not charted: {}. Reaching up to {} {}, they would flatten the leaders.",
        names.join(", "),
        metric.curve_format(far),
        metric.curve_unit(),
    ))
}
