// The nightly table of the Performance tab.

use std::rc::Rc;

use cranpose_ui::{
    composable, text::FontWeight, Color, Column, ColumnSpec, LinearArrangement, Modifier, Row,
    RowSpec, Text,
};

use super::{figure, text_style, Palette, PerfRun};

/// Each scenario's medians for one `start`: main's value over the
/// release's, coloured by its verdicts.
#[composable]
pub(super) fn NightlyTable(palette: Palette, run: Rc<PerfRun>, start: &'static str) {
    let headers = ["scenario", "fps", "change", "CPU ms", "latency ms"]
        .map(|header| TableCell::new(header.to_string(), palette.muted));
    TableRow(palette, headers.to_vec(), true);
    for scenario in run.scenarios.keys() {
        let entry = run.start(scenario, start).cloned().unwrap_or_default();
        let verdict = |metric: &str| palette.verdict(entry.verdicts.get(metric));
        let pair = |metric: &str| TableCell {
            text: figure(run.median(scenario, start, 1, metric)),
            under: Some(figure(run.median(scenario, start, 0, metric))),
            color: verdict(metric),
        };
        let change = match (
            run.median(scenario, start, 0, "fps"),
            run.median(scenario, start, 1, "fps"),
        ) {
            (Some(before), Some(after)) if before > 0.0 => {
                format!("{:+.1}%", (after - before) / before * 100.0)
            }
            _ => "–".to_string(),
        };
        let name = if entry.confirmed.is_empty() {
            TableCell::new(scenario.clone(), palette.text)
        } else {
            TableCell::new(format!("{scenario} ⚠"), palette.worse)
        };
        let cells = vec![
            name,
            pair("fps"),
            TableCell::new(change, verdict("fps")),
            pair("cpu_ms_per_frame"),
            pair("desired_to_present_p50_ms"),
        ];
        TableRow(palette, cells, false);
    }
}

/// A table cell: its text, and the release's value under main's.
#[derive(Clone, Debug, PartialEq)]
struct TableCell {
    text: String,
    under: Option<String>,
    color: Color,
}

impl TableCell {
    fn new(text: String, color: Color) -> Self {
        Self {
            text,
            under: None,
            color,
        }
    }
}

#[composable]
fn TableRow(palette: Palette, cells: Vec<TableCell>, header: bool) {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            for (index, cell) in cells.iter().enumerate() {
                let cell = cell.clone();
                Column(
                    Modifier::empty().weight(if index == 0 { 1.4 } else { 1.0 }),
                    ColumnSpec::default(),
                    move || {
                        Text(
                            cell.text.clone(),
                            Modifier::empty(),
                            text_style(
                                13.0,
                                cell.color,
                                (header || index == 0).then_some(FontWeight::BOLD),
                            ),
                        );
                        if let Some(under) = cell.under.clone() {
                            Text(
                                under,
                                Modifier::empty(),
                                text_style(11.0, palette.muted, None),
                            );
                        }
                    },
                );
            }
        },
    );
}
