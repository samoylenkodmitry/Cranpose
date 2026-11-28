// The framework rows of the Performance tab: a name and a bar for each.

use std::rc::Rc;

use cranpose_animation::animateFloatAsState;
use cranpose_core::key;
use cranpose_foundation::SemanticsWidgetRole;
use cranpose_services::local_uri_handler;
use cranpose_ui::{
    composable, text::FontWeight, Alignment, Box, BoxSpec, Brush, Canvas, Color, CornerRadii,
    DrawScope, LinearArrangement, Modifier, PointerEventKind, PointerInputScope, Rect, Row,
    RowSpec, Stroke, Text, TextOptions, TextOverflow, TextWithOptions, VerticalAlignment,
};

use super::{
    figure,
    focus::{faded, Focus},
    launch_chart::LaunchLine,
    motion::{fade, settle, GONE},
    text_style, Dot, Metric, Palette, PerfRun, PerfSubject, Selection, SOURCE_URL,
};

// A row holds two lines of its name, which a narrow card wraps.
const ROW_HEIGHT: f32 = 32.0;
const BAR_HEIGHT: f32 = 22.0;

/// Every subject's metric of the `selection` as a bar in its line's colour,
/// best first, alphabetical among equals: its dot, its name, which opens its
/// source at the commit the run measured, and the bar with its value, or why
/// it has none. The pointer over a row hovers it in `focus`; a click picks it.
#[composable]
pub(super) fn SubjectBars(
    palette: Palette,
    focus: Focus,
    run: Rc<PerfRun>,
    subjects: Rc<Vec<(usize, PerfSubject)>>,
    selection: Selection,
    lines: Vec<LaunchLine>,
) {
    let commit = run.main.clone().unwrap_or_else(|| "main".to_string());
    let values: Vec<Option<f64>> = subjects
        .iter()
        .map(|(index, _)| ranked_value(&run, &selection, &lines, *index))
        .collect();
    let full = selection.metric.full_scale(&values);
    let mut order: Vec<usize> = (0..subjects.len()).collect();
    order.sort_by(|&a, &b| selection.metric.best_first(values[a], values[b]));
    let rows = order.len() as f32;
    // The rows stand at places of their own, so that each moves to its new
    // place when the metric changes their order.
    Box(
        Modifier::empty().fill_max_width().height(rows * ROW_HEIGHT),
        BoxSpec::default(),
        move || {
            let ctx = BarContext {
                palette,
                run: &run,
                selection: &selection,
                commit: &commit,
                full,
            };
            for (position, subject_at) in order.iter().copied().enumerate() {
                let (index, subject) = &subjects[subject_at];
                let line = lines.iter().find(|line| line.subject == *index);
                let row = build_subject_row(&ctx, (*index, subject), line, values[subject_at]);
                key(*index, || SubjectBar(palette, focus, row, position));
            }
        },
    );
}

struct BarContext<'a> {
    palette: Palette,
    run: &'a PerfRun,
    selection: &'a Selection,
    commit: &'a str,
    full: f64,
}

fn ranked_value(
    run: &PerfRun,
    selection: &Selection,
    lines: &[LaunchLine],
    subject: usize,
) -> Option<f64> {
    let disturbed = lines
        .iter()
        .find(|line| line.subject == subject)
        .is_some_and(|line| line.disturbed);
    if disturbed && selection.metric != Metric::AppSize {
        return None;
    }
    // A board's bars rank by the frames its panel can show.
    let key = if run.protocol.panel_hz.is_some() && selection.metric == Metric::Fps {
        SHOWN_FPS
    } else {
        selection.metric.key()
    };
    run.median(&selection.scenario, selection.start, subject, key)
}

// A board run's frames a second that its panel can show: each second's drawn
// frames up to the panel's rate.
const SHOWN_FPS: &str = "shown_fps";

fn build_subject_row(
    ctx: &BarContext<'_>,
    (index, subject): (usize, &PerfSubject),
    line: Option<&LaunchLine>,
    metric_value: Option<f64>,
) -> SubjectRow {
    let disturbed = line.is_some_and(|line| line.disturbed);
    // A subject with legs but without this metric gets a dash;
    // one that stayed disturbed shows its mark; one with no legs says why.
    // A subject that wrote no frame it moved on to has MangoHud's count alone.
    let measured = !disturbed
        && ["fps", "mangohud_fps"].iter().any(|key| {
            ctx.run
                .median(&ctx.selection.scenario, ctx.selection.start, index, key)
                .is_some()
        });
    let (share, value) = match metric_value {
        Some(value) => (
            Some(if ctx.full > 0.0 {
                (value / ctx.full) as f32
            } else {
                0.0
            }),
            beside_other_rate(ctx, index, ctx.selection.metric.format(value)),
        ),
        None if disturbed => (None, "disturbed".to_string()),
        None if measured => (None, beside_other_rate(ctx, index, "–".to_string())),
        None => (
            None,
            line.and_then(|line| line.failure.clone())
                .unwrap_or_else(|| "no frames".to_string()),
        ),
    };
    SubjectRow {
        subject: index,
        label: subject.label.clone(),
        url: subject
            .source
            .as_ref()
            .map(|source| format!("{SOURCE_URL}/{}/{source}", ctx.commit)),
        color: line.map_or(ctx.palette.baseline, |line| line.color),
        emphasized: line.is_some_and(|line| line.emphasized),
        disturbed,
        share,
        value,
    }
}

// On a board run, the frames drawn a second beside the frames shown, the
// rate the bar stands for. On a run MangoHud counted presents in, the app's
// frame rate with MangoHud's for the same launches beside it; where a
// launch's presents and the frames it moved on to disagree, either rate says
// so, MangoHud's with the app's beside it.
fn beside_other_rate(ctx: &BarContext<'_>, subject: usize, value: String) -> String {
    let run = ctx.run;
    let (scenario, start) = (ctx.selection.scenario.as_str(), ctx.selection.start);
    let rate = |metric: Metric| figure(run.median(scenario, start, subject, metric.key()));
    if run.protocol.panel_hz.is_some() && ctx.selection.metric == Metric::Fps {
        return format!("{} drawn · {value} shown", rate(Metric::Fps));
    }
    if run.protocol.mangohud.is_none() {
        return value;
    }
    let mismatch = run.count_mismatch(scenario, start, subject);
    let mark = mismatch.map_or_else(String::new, |mark| format!(" · {mark}"));
    // Why MangoHud logged no present, and that the subject drew on the CPU.
    let notes = run.notes(scenario, start, subject);
    let noted = |text: String| {
        notes
            .iter()
            .fold(text, |text, note| format!("{text} · {note}"))
    };
    let counted = run
        .median(scenario, start, subject, Metric::MangoHudFps.key())
        .is_some();
    match ctx.selection.metric {
        Metric::Fps | Metric::MangoHudFps if !counted => noted(value),
        Metric::Fps => noted(format!(
            "{value} · MangoHud {}{mark}",
            rate(Metric::MangoHudFps)
        )),
        Metric::MangoHudFps if mismatch.is_some() => {
            noted(format!("{value}{mark}, {} updates/s", rate(Metric::Fps)))
        }
        Metric::MangoHudFps => noted(value),
        _ => value,
    }
}

// What a subject's row shows: its bar fills `share` of the full length, and a
// subject with no share shows in the bar's place why it has no value.
#[derive(Clone, Debug, PartialEq)]
struct SubjectRow {
    subject: usize,
    label: String,
    url: Option<String>,
    color: Color,
    emphasized: bool,
    disturbed: bool,
    share: Option<f32>,
    value: String,
}

/// One subject's row at `position` from the top: the dot of its line's colour,
/// its name, and its value on a bar that fades into its colour. The row fades
/// while `focus` hovers or picks other subjects; a picked bar has an outline.
#[composable]
fn SubjectBar(palette: Palette, focus: Focus, row: SubjectRow, position: usize) {
    let weight = animateFloatAsState(focus.weight(row.subject), fade(), "bar_weight").value();
    let picked = focus.picked.get().contains(&row.subject);
    let outline = animateFloatAsState(f32::from(picked), fade(), "bar_picked").value();
    let place = animateFloatAsState(position as f32 * ROW_HEIGHT, settle(), "bar_place").value();
    let length = animateFloatAsState(row.share.unwrap_or(0.0), settle(), "bar_length").value();
    Row(
        row_hover(
            Modifier::empty()
                .offset(0.0, place)
                .fill_max_width()
                .height(ROW_HEIGHT),
            focus,
            row.subject,
        ),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Dot(faded(row.color, weight));
            RowName(palette, row.clone(), weight);
            let look = BarLook {
                weight,
                length,
                outline,
            };
            RowBar(palette, focus, row.clone(), look, picked);
        },
    );
}

// How a bar shows: the share of its opacity it keeps, how much of the full
// length it fills, and how much of its outline a pick has drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
struct BarLook {
    weight: f32,
    length: f32,
    outline: f32,
}

// A subject's name, which opens its source where it has one.
#[composable]
fn RowName(palette: Palette, row: SubjectRow, weight: f32) {
    let uri_handler = local_uri_handler().current();
    let name = Modifier::empty().weight(0.44);
    let name = match row.url {
        Some(url) => name.clickable(move |_| {
            if let Err(error) = uri_handler.open_uri(&url) {
                log::error!("Could not open {url}: {error:#}");
            }
        }),
        None => name,
    };
    TextWithOptions(
        row.label,
        name,
        text_style(
            12.0,
            faded(palette.text, weight),
            row.emphasized.then_some(FontWeight::BOLD),
        ),
        TextOptions {
            overflow: TextOverflow::Ellipsis,
            max_lines: Some(2),
            ..TextOptions::default()
        },
    );
}

// A subject's value on its bar, which a click picks; with no bar, why it has
// no value.
#[composable]
fn RowBar(palette: Palette, focus: Focus, row: SubjectRow, look: BarLook, picked: bool) {
    let subject = row.subject;
    let toggling = if row.share.is_some() {
        Modifier::empty().toggleable(
            picked,
            Some(format!("{} on the chart", row.label)),
            Some(SemanticsWidgetRole::Checkbox),
            move |_| focus.toggle(subject),
        )
    } else {
        Modifier::empty()
    };
    Box(
        toggling.weight(0.56).height(BAR_HEIGHT),
        BoxSpec::new().content_alignment(Alignment::CENTER_START),
        move || {
            if row.share.is_none() {
                let note_color = if row.disturbed {
                    palette.open
                } else {
                    palette.muted
                };
                TextWithOptions(
                    row.value.clone(),
                    Modifier::empty().fill_max_width().padding_horizontal(4.0),
                    text_style(11.0, faded(note_color, look.weight), None),
                    TextOptions {
                        overflow: TextOverflow::Ellipsis,
                        max_lines: Some(1),
                        ..TextOptions::default()
                    },
                );
                return;
            }
            let color = row.color;
            Canvas(Modifier::empty().fill_max_size(), move |scope| {
                draw_bar(scope, palette, color, look);
            });
            Text(
                row.value.clone(),
                Modifier::empty().padding_horizontal(9.0),
                text_style(
                    11.5,
                    faded(palette.text, look.weight),
                    Some(FontWeight::BOLD),
                ),
            );
        },
    );
}

// A bar: its track, its fill that fades into `color`, and the outline of a
// pick.
fn draw_bar(scope: &mut dyn DrawScope, palette: Palette, color: Color, look: BarLook) {
    let size = scope.size();
    let radius = CornerRadii::uniform(size.height / 2.0);
    let track = Rect {
        x: 0.0,
        y: 0.0,
        width: size.width,
        height: size.height,
    };
    let weight = look.weight;
    scope.draw_round_rect_at(
        track,
        Brush::solid(faded(palette.background, weight)),
        radius,
    );
    let width = (size.width * look.length.clamp(0.0, 1.0)).max(size.height);
    scope.draw_round_rect_at(
        Rect { width, ..track },
        Brush::horizontal_gradient(
            vec![faded(color, 0.35 * weight), faded(color, weight)],
            0.0,
            width,
        ),
        radius,
    );
    if look.outline > GONE {
        scope.draw_round_rect_at_stroked(
            Rect {
                x: 0.75,
                y: 0.75,
                width: size.width - 1.5,
                height: size.height - 1.5,
            },
            Brush::solid(faded(color, look.outline)),
            radius,
            Stroke::new(1.5),
        );
    }
}

// Hovers `subject` in `focus` while the pointer is over the row.
fn row_hover(modifier: Modifier, focus: Focus, subject: usize) -> Modifier {
    modifier.pointer_input(subject, move |scope: PointerInputScope| async move {
        scope
            .await_pointer_event_scope(|events| async move {
                loop {
                    match events.await_pointer_event().await.kind {
                        PointerEventKind::Enter
                        | PointerEventKind::Move
                        | PointerEventKind::Down => focus.hovered.set(Some(subject)),
                        PointerEventKind::Exit
                            if focus.hovered.get_non_reactive() == Some(subject) =>
                        {
                            focus.hovered.set(None);
                        }
                        _ => {}
                    }
                }
            })
            .await;
    })
}
