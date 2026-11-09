// The curves of a launch chart over their grid: the fades of a hover and of
// picked bars, a scale that settles on what is drawn, and a tooltip where the
// pointer reads.

use std::rc::Rc;

use cranpose_animation::{animateFloatAsState, animateValueAsState};
use cranpose_core::{key, MutableState};
use cranpose_ui::{
    composable, text::FontWeight, Alignment, Box, BoxSpec, Brush, Color, Column, ColumnSpec,
    DrawScope, DrawStyle, LinearArrangement, Modifier, Path, Point, PointerEventKind,
    PointerInputScope, Row, RowSpec, Stroke, Text, VerticalAlignment,
};

use super::{
    chart_view::{ChartHover, ChartView, Plotted},
    focus::{faded, Focus, DIMMED_WEIGHT},
    launch_chart::LaunchCurve,
    motion::{fade, settle, GONE},
    plot::{Axis, Plot, CHART_HEIGHT},
    text_style, Curve, Dot, Palette, CHART_STEP_MS,
};

// The opacity of a curve's band, and of the lifted curve's.
const BAND_ALPHA: f32 = 0.10;
const LIFTED_BAND_ALPHA: f32 = 0.18;

// How a curve shows: the share of its opacity it keeps, and how far the
// pointer has lifted it over the others.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CurveLook {
    weight: f32,
    lift: f32,
}

// The look of `subject`'s curve, settling at its target: gone while the chart
// does not draw it, dimmed while another drawn subject is hovered, lifted
// while it is.
fn look(view: &ChartView, subject: usize, hovered: Option<usize>) -> CurveLook {
    key(subject, || {
        let shown = view.shown.contains(&subject);
        let dimmed = hovered.is_some_and(|other| other != subject && view.shown.contains(&other));
        let weight = if !shown {
            0.0
        } else if dimmed {
            DIMMED_WEIGHT
        } else {
            1.0
        };
        CurveLook {
            weight: animateFloatAsState(weight, fade(), "curve_weight").value(),
            lift: animateFloatAsState(
                f32::from(shown && hovered == Some(subject)),
                fade(),
                "curve_lift",
            )
            .value(),
        }
    })
}

// One value the pointer reads, on the curve of a subject.
#[derive(Clone, Debug, PartialEq)]
struct Reading {
    label: String,
    color: Color,
    value: f64,
    text: String,
}

// What the pointer reads at `at`: the curve it holds, or with picked bars all
// the curves drawn, best first.
fn readings(plotted: &Plotted, view: &ChartView, at: &ChartHover) -> Vec<Reading> {
    let metric = plotted.metric;
    let mut readings: Vec<Reading> = plotted
        .curves
        .iter()
        .filter(|curve| {
            view.shown.contains(&curve.line.subject)
                && at.subject.is_none_or(|held| held == curve.line.subject)
        })
        .filter_map(|curve| {
            let value = curve
                .median
                .get(at.step)
                .copied()
                .filter(|value| value.is_finite())?;
            Some(Reading {
                label: curve.line.label.clone(),
                color: curve.line.color,
                value,
                text: format!("{} {}", metric.curve_format(value), metric.curve_unit()),
            })
        })
        .collect();
    readings.sort_by(|a, b| {
        if metric.curve() == Curve::Frames {
            b.value.total_cmp(&a.value)
        } else {
            a.value.total_cmp(&b.value)
        }
    });
    readings
}

// A path through `values`, one every chart step from the launch, broken
// where no launch read a value.
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

// The stretches of steps over which `values` reads a value.
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

// Each curve: its band from the lowest to the highest launch, then its median
// line, each value capped at the top. The lifted curve is drawn last, else
// Cranpose's.
fn draw_curves(scope: &mut dyn DrawScope, plotted: &Plotted, looks: &[CurveLook], axis: Axis) {
    let size = scope.size();
    let at = |step: usize, value: f64| plotted.point(size, step, value, axis);
    let drawn = || {
        plotted
            .curves
            .iter()
            .zip(looks)
            .filter(|(_, look)| look.weight > GONE)
    };
    for (curve, look) in drawn() {
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
        let alpha = BAND_ALPHA + look.lift * (LIFTED_BAND_ALPHA - BAND_ALPHA);
        scope.draw_path(
            &band,
            Brush::solid(faded(curve.line.color, look.weight * alpha)),
            DrawStyle::Fill,
        );
    }
    let lifted = looks.iter().any(|look| look.lift > GONE);
    let front = |curve: &LaunchCurve, look: &CurveLook| {
        if lifted {
            look.lift > GONE
        } else {
            curve.line.emphasized
        }
    };
    for (curve, look) in drawn()
        .filter(|(curve, look)| !front(curve, look))
        .chain(drawn().filter(|(curve, look)| front(curve, look)))
    {
        let mut line = Path::new();
        polyline(&mut line, curve.median.iter().copied().enumerate(), at);
        let base = if curve.line.emphasized { 2.5 } else { 1.5 };
        scope.draw_path(
            &line,
            Brush::solid(faded(curve.line.color, look.weight)),
            DrawStyle::Stroke(Stroke::new(base + look.lift * (3.0 - base))),
        );
    }
}

// The guide through the time the pointer reads, and a marker on each curve it
// reads there.
fn draw_hover(
    scope: &mut dyn DrawScope,
    palette: Palette,
    plotted: &Plotted,
    axis: Axis,
    step: usize,
    marks: &[(f64, Color)],
) {
    let size = scope.size();
    let guide = plotted.point(size, step, 0.0, axis).x;
    scope.draw_line(
        Brush::solid(palette.muted.with_alpha(0.5)),
        Point { x: guide, y: 0.0 },
        Point {
            x: guide,
            y: size.height,
        },
        Stroke::new(1.0),
    );
    for (value, color) in marks {
        let at = plotted.point(size, step, *value, axis);
        scope.draw_circle(Brush::solid(palette.surface), at, 6.0);
        scope.draw_circle(Brush::solid(*color), at, 4.0);
    }
}

// Reads the curves at the pointer as it moves over the chart, and names the
// curve it holds in `focus` while no bars are picked.
fn chart_pointer(
    modifier: Modifier,
    focus: Focus,
    hover: MutableState<Option<ChartHover>>,
    latest: MutableState<(Rc<Plotted>, Rc<ChartView>)>,
) -> Modifier {
    modifier.pointer_input((), move |scope: PointerInputScope| async move {
        scope
            .await_pointer_event_scope(|events| async move {
                loop {
                    let event = events.await_pointer_event().await;
                    let (plotted, view) = latest.get_non_reactive();
                    let reached = match event.kind {
                        PointerEventKind::Enter
                        | PointerEventKind::Move
                        | PointerEventKind::Down => {
                            let held = hover.get_non_reactive().and_then(|at| at.subject);
                            view.read(&plotted, event.position, events.size(), held)
                        }
                        PointerEventKind::Exit => None,
                        _ => continue,
                    };
                    hover.set(reached);
                    if !view.picking {
                        focus.hovered.set(reached.and_then(|at| at.subject));
                    }
                }
            })
            .await;
    })
}

// The curves of `plotted` as `view` draws them, over a grid. The hovered
// subject keeps its colour and the other curves dim; a tooltip names what the
// pointer reads and the time under it.
#[composable]
pub(super) fn CurvePlot(palette: Palette, focus: Focus, plotted: Rc<Plotted>, view: Rc<ChartView>) {
    let hover: MutableState<Option<ChartHover>> = cranpose_core::rememberMutableStateOf(|| None);
    let latest = cranpose_core::rememberUpdatedState((Rc::clone(&plotted), Rc::clone(&view)));
    let axis = Axis {
        floor: animateValueAsState(view.axis.floor, settle(), "chart_floor").value(),
        top: animateValueAsState(view.axis.top, settle(), "chart_top").value(),
        step: view.axis.step,
    };
    let hovered = focus.hovered.get();
    let looks: Vec<CurveLook> = plotted
        .curves
        .iter()
        .map(|curve| look(&view, curve.line.subject, hovered))
        .collect();
    let pointed = hover.get().map(|at| (at, readings(&plotted, &view, &at)));
    Box(
        chart_pointer(
            Modifier::empty().fill_max_width().height(CHART_HEIGHT),
            focus,
            hover,
            latest,
        ),
        BoxSpec::default(),
        move || {
            let (drawn, looks) = (Rc::clone(&plotted), looks.clone());
            let marks: Option<(usize, Vec<(f64, Color)>)> = pointed.as_ref().map(|(at, read)| {
                (
                    at.step,
                    read.iter().map(|read| (read.value, read.color)).collect(),
                )
            });
            Plot(palette, axis, move |scope| {
                draw_curves(scope, &drawn, &looks, axis);
                if let Some((step, marks)) = &marks {
                    draw_hover(scope, palette, &drawn, axis, *step, marks);
                }
            });
            if let Some((at, read)) = pointed.clone().filter(|(_, read)| !read.is_empty()) {
                let time = format!("{:.2} s", at.step as f64 * CHART_STEP_MS / 1000.0);
                Tooltip(palette, time, read, at);
            }
        },
    );
}

// The gap between a tooltip and the point it reads, and the room its lines
// and padding need.
const TOOLTIP_GAP: f32 = 10.0;
const TOOLTIP_LINE: f32 = 16.0;
const TOOLTIP_PADDING: f32 = 12.0;

// The `time` and each of `readings`, beside the point the pointer reads: left
// of it in the chart's right half, over it where there is room.
#[composable]
fn Tooltip(palette: Palette, time: String, readings: Vec<Reading>, at: ChartHover) {
    let height = TOOLTIP_PADDING + TOOLTIP_LINE * (readings.len() + 1) as f32;
    let right = at.anchor.x > at.width / 2.0;
    let x = if right {
        at.anchor.x - at.width - TOOLTIP_GAP
    } else {
        at.anchor.x + TOOLTIP_GAP
    };
    let above = at.anchor.y > height + TOOLTIP_GAP;
    let y = if above {
        at.anchor.y - height - TOOLTIP_GAP
    } else {
        at.anchor.y + TOOLTIP_GAP
    }
    .clamp(0.0, (CHART_HEIGHT - height).max(0.0));
    Box(
        Modifier::empty().fill_max_size(),
        BoxSpec::new().content_alignment(if right {
            Alignment::TOP_END
        } else {
            Alignment::TOP_START
        }),
        move || {
            let (time, readings) = (time.clone(), readings.clone());
            Column(
                Modifier::empty()
                    .offset(x, y)
                    .background(palette.text.with_alpha(0.92))
                    .rounded_corners(8.0)
                    .padding_symmetric(8.0, 5.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(2.0)),
                move || {
                    Text(
                        time.clone(),
                        Modifier::empty(),
                        text_style(10.5, palette.surface.with_alpha(0.7), None),
                    );
                    for reading in readings.clone() {
                        Row(
                            Modifier::empty(),
                            RowSpec::new()
                                .horizontal_arrangement(LinearArrangement::SpacedBy(6.0))
                                .vertical_alignment(VerticalAlignment::CenterVertically),
                            move || {
                                Dot(reading.color);
                                Text(
                                    reading.label.clone(),
                                    Modifier::empty(),
                                    text_style(11.0, palette.surface, Some(FontWeight::BOLD)),
                                );
                                Text(
                                    reading.text.clone(),
                                    Modifier::empty(),
                                    text_style(11.0, palette.surface, None),
                                );
                            },
                        );
                    }
                },
            );
        },
    );
}
