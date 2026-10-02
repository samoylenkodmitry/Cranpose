use std::sync::OnceLock;

use cranpose_core::{remember, rememberMutableStateOf, with_key, MutableState};
use cranpose_ui::{
    composable, Box, BoxSpec, Button, ButtonSpec, Color, Column, ColumnSpec, LinearArrangement,
    Modifier, Row, RowSpec, ScrollState, Text, TextStyle,
};

use super::markdown::MarkdownDocument;

const GUIDE: &str = include_str!("../../../../docs/guide.md");
const GUIDE_URL: &str = "https://github.com/samoylenkodmitry/Cranpose/blob/main/docs/guide.md";

struct Chapter {
    title: &'static str,
    body: &'static str,
}

fn chapters() -> &'static [Chapter] {
    static CHAPTERS: OnceLock<Vec<Chapter>> = OnceLock::new();
    CHAPTERS.get_or_init(|| {
        GUIDE
            .split("\n## ")
            .skip(1)
            .filter_map(|section| {
                let (title, body) = section.split_once('\n')?;
                Some(Chapter { title, body })
            })
            .collect()
    })
}

#[composable]
fn ChapterPicker(selected: MutableState<usize>) {
    let scroll = remember(|| ScrollState::new(0.0)).with(|state| *state);
    Row(
        Modifier::empty()
            .fill_max_width()
            .clip_to_bounds()
            .horizontal_scroll(scroll, false),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            for (index, chapter) in chapters().iter().enumerate() {
                let active = selected.get() == index;
                Button(
                    Modifier::empty()
                        .background(if active {
                            Color(0.2, 0.45, 0.9, 1.0)
                        } else {
                            Color(0.12, 0.16, 0.24, 1.0)
                        })
                        .rounded_corners(8.0)
                        .padding(12.0)
                        .semantics(move |config| config.selected = Some(active)),
                    ButtonSpec::default(),
                    move || selected.set(index),
                    move || {
                        Text(chapter.title, Modifier::empty(), TextStyle::default());
                    },
                );
            }
        },
    );
}

#[composable]
pub(super) fn DocumentationTab() {
    let selected = rememberMutableStateOf(|| 0usize);
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.06, 0.08, 0.14, 1.0))
            .rounded_corners(16.0)
            .padding(16.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
        move || {
            Text(
                "Cranpose documentation",
                Modifier::empty().heading(),
                TextStyle::default(),
            );
            ChapterPicker(selected);
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .weight(1.0)
                    .clip_to_bounds(),
                BoxSpec::default(),
                move || {
                    if let Some(chapter) = chapters().get(selected.get()) {
                        with_key(&chapter.title, || MarkdownDocument(chapter.body, GUIDE_URL));
                    }
                },
            );
        },
    );
}
