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
    focus::{faded, Focus},
    launch_chart::LaunchLine,
    motion::{fade, settle, GONE},
    text_style, Dot, Palette, PerfRun, PerfSubject, Selection, SOURCE_URL,
};

const ROW_HEIGHT: f32 = 22.0;
const ROW_GAP: f32 = 10.0;

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
    let Selection {
        scenario,
        start,
        metric,
    } = selection;
    let commit = run.main.clone().unwrap_or_else(|| "main".to_string());
    let values: Vec<Option<f64>> = subjects
        .iter()
        .map(|(index, _)| run.median(&scenario, start, *index, metric.key()))
        .collect();
    let full = metric.full_scale(&values);
    let mut order: Vec<usize> = (0..subjects.len()).collect();
    order.sort_by(|&a, &b| metric.best_first(values[a], values[b]));
    let rows = order.len() as f32;
    // The rows stand at places of their own, so that each moves to its new
    // place when the metric changes their order.
    Box(
        Modifier::empty()
            .fill_max_width()
            .height((rows * (ROW_HEIGHT + ROW_GAP) - ROW_GAP).max(0.0)),
        BoxSpec::default(),
        move || {
            for (position, subject_at) in order.iter().copied().enumerate() {
                let (index, subject) = &subjects[subject_at];
                let line = lines.iter().find(|line| line.subject == *index);
                // A subject with legs but without this metric gets a dash;
                // one with no legs says why.
                let measured = run.median(&scenario, start, *index, "fps").is_some();
                let (share, value) = match values[subject_at] {
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
                let row = SubjectRow {
                    subject: *index,
                    label: subject.label.clone(),
                    url: subject
                        .source
                        .as_ref()
                        .map(|source| format!("{SOURCE_URL}/{commit}/{source}")),
                    color: line.map_or(palette.baseline, |line| line.color),
                    emphasized: line.is_some_and(|line| line.emphasized),
                    share,
                    value,
                };
                key(*index, || SubjectBar(palette, focus, row, position));
            }
        },
    );
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
    let place = animateFloatAsState(
        position as f32 * (ROW_HEIGHT + ROW_GAP),
        settle(),
        "bar_place",
    )
    .value();
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
    let style = text_style(
        12.0,
        faded(palette.text, weight),
        row.emphasized.then_some(FontWeight::BOLD),
    );
    match row.url {
        Some(url) => {
            Text(
                row.label,
                name.clickable(move |_| {
                    if let Err(error) = uri_handler.open_uri(&url) {
                        log::error!("Could not open {url}: {error:#}");
                    }
                }),
                style,
            );
        }
        None => {
            Text(row.label, name, style);
        }
    }
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
        toggling.weight(0.56).height(ROW_HEIGHT),
        BoxSpec::new().content_alignment(Alignment::CENTER_START),
        move || {
            if row.share.is_none() {
                TextWithOptions(
                    row.value.clone(),
                    Modifier::empty().fill_max_width().padding_horizontal(4.0),
                    text_style(11.0, faded(palette.muted, look.weight), None),
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
