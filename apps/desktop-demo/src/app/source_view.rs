use std::rc::Rc;

use cranpose::LazyItems;
use cranpose_core::{rememberMutableStateOf, MutableState};
use cranpose_foundation::lazy::{rememberLazyListState, LazyListScope};
use cranpose_services::local_http_client;
use cranpose_ui::{
    composable,
    text::{AnnotatedString, SpanStyle, TextUnit},
    widgets::{LazyColumn, LazyColumnSpec},
    Button, ButtonSpec, Color, Column, ColumnSpec, LinearArrangement, Modifier, Row, RowSpec, Text,
    TextStyle,
};

use super::{
    highlight::{language_from_fence, Language},
    highlight_theme::highlight_lines,
    DemoTab,
};

/// The floating toggle sits in the tab strip's top padding, above the tab
/// buttons, so it must stay short enough not to reach them: a control that
/// overlapped a tab would take the click that belongs to it.
const COMPACT_TOGGLE_PADDING: f32 = 3.0;
const COMPACT_TOGGLE_FONT_SP: f32 = 11.0;

const REPOSITORY: &str = "https://raw.githubusercontent.com/samoylenkodmitry/cranpose";

pub(crate) fn source_path(tab: DemoTab) -> &'static str {
    tab.source_path()
}

pub(crate) fn source_ref() -> &'static str {
    env!("CRANPOSE_SOURCE_REF")
}

fn short_ref() -> &'static str {
    let reference = source_ref();
    let looks_like_a_commit =
        reference.len() == 40 && reference.chars().all(|c| c.is_ascii_hexdigit());
    if looks_like_a_commit {
        &reference[..7]
    } else {
        reference
    }
}

fn source_url(tab: DemoTab) -> String {
    format!("{REPOSITORY}/{}/{}", source_ref(), source_path(tab))
}

#[derive(Clone, PartialEq)]
enum SourceState {
    Loading,
    Ready(Rc<Vec<Rc<AnnotatedString>>>),
    Error(String),
}

fn language_for(path: &str) -> Language {
    match path.rsplit_once('.') {
        Some((_, extension)) => language_from_fence(extension),
        None => Language::Plain,
    }
}

fn highlighted_lines(path: &str, body: &str) -> Rc<Vec<Rc<AnnotatedString>>> {
    Rc::new(
        highlight_lines(language_for(path), body)
            .into_iter()
            .map(Rc::new)
            .collect(),
    )
}

#[allow(non_snake_case)]
#[composable]
pub(crate) fn SourceToggleButton(showing: MutableState<bool>, modifier: Modifier, compact: bool) {
    let label = if showing.get() {
        "Hide source"
    } else {
        "Show source"
    };
    let padding = if compact {
        COMPACT_TOGGLE_PADDING
    } else {
        10.0
    };
    let style = if compact {
        TextStyle {
            span_style: SpanStyle {
                font_size: TextUnit::Sp(COMPACT_TOGGLE_FONT_SP),
                ..Default::default()
            },
            ..Default::default()
        }
    } else {
        TextStyle::default()
    };
    Button(
        modifier.rounded_corners(12.0).padding(padding),
        ButtonSpec::default(),
        move || showing.set(!showing.get()),
        move || {
            Text(label, Modifier::empty(), style.clone());
        },
    );
}

#[allow(non_snake_case)]
#[composable]
pub(crate) fn SourcePanel(tab: DemoTab) {
    let state = rememberMutableStateOf(|| SourceState::Loading);
    let http_client = local_http_client().current();
    let list_state = rememberLazyListState();

    cranpose_core::LaunchedEffect(tab, move |scope| {
        state.set(SourceState::Loading);
        let client = http_client.clone();
        let url = source_url(tab);
        scope.launch_background(
            move |_token| async move {
                client
                    .get_text(&url)
                    .await
                    .map_err(|error| format!("could not fetch the source: {error}"))
            },
            move |result| match result {
                Ok(body) => state.set(SourceState::Ready(highlighted_lines(
                    source_path(tab),
                    &body,
                ))),
                Err(error) => state.set(SourceState::Error(error)),
            },
        );
    });

    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.06, 0.07, 0.10, 0.96))
            .rounded_corners(14.0)
            .padding(14.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                format!("{} @ {}", source_path(tab), short_ref()),
                Modifier::empty(),
                TextStyle::default(),
            );

            match state.get() {
                SourceState::Loading => {
                    Text("Loading source...", Modifier::empty(), TextStyle::default());
                }
                SourceState::Error(message) => {
                    Text(message, Modifier::empty(), TextStyle::default());
                }
                SourceState::Ready(lines) => {
                    let count = lines.len();
                    LazyColumn(
                        Modifier::empty().fill_max_size(),
                        list_state,
                        LazyColumnSpec::new()
                            .vertical_arrangement(LinearArrangement::SpacedBy(1.0)),
                        move |scope| {
                            let lines = lines.clone();
                            scope.items(
                                LazyItems::new(count).key(|index: usize| index as u64),
                                move |index| {
                                    SourceLine(index + 1, lines[index].clone());
                                },
                            );
                        },
                    );
                }
            }
        },
    );
}

#[allow(non_snake_case)]
#[composable]
fn SourceLine(number: usize, line: Rc<AnnotatedString>) {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(10.0)),
        move || {
            Text(
                format!("{number:>4}"),
                Modifier::empty(),
                TextStyle {
                    span_style: SpanStyle {
                        color: Some(Color(0.38, 0.42, 0.50, 1.0)),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
            Text(line.clone(), Modifier::empty(), TextStyle::default());
        },
    );
}
