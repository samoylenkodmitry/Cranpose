// The framework rows of the Performance tab: a name and a bar for each.

use std::rc::Rc;

use cranpose_services::local_uri_handler;
use cranpose_ui::{
    composable, text::FontWeight, Alignment, Box, BoxSpec, Brush, Canvas, Color, LinearArrangement,
    Modifier, Row, RowSpec, Text, TextOptions, TextOverflow, TextWithOptions, VerticalAlignment,
};

use super::{
    launch_chart::LaunchLine, text_style, Dot, Metric, Palette, PerfRun, PerfSubject, SOURCE_URL,
};

/// Every subject's `metric` for `scenario` and `start` as a bar in its line's
/// colour, best first, alphabetical among equals: its dot, its name, which
/// opens its source at the commit the run measured, and the bar with its
/// value, or why it has none.
#[composable]
pub(super) fn SubjectBars(
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
