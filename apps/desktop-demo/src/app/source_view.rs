use std::rc::Rc;

use cranpose::LazyItems;
use cranpose_core::{rememberMutableStateOf, MutableState};
use cranpose_foundation::lazy::{rememberLazyListState, LazyListScope};
use cranpose_services::local_http_client;
use cranpose_ui::{
    composable,
    widgets::{LazyColumn, LazyColumnSpec},
    Button, ButtonSpec, Color, Column, ColumnSpec, LinearArrangement, Modifier, Row, RowSpec, Text,
    TextStyle,
};

use super::DemoTab;

const REPOSITORY: &str = "https://raw.githubusercontent.com/samoylenkodmitry/cranpose";

pub(crate) fn source_path(tab: DemoTab) -> &'static str {
    match tab {
        DemoTab::Counter
        | DemoTab::CompositionLocal
        | DemoTab::Async
        | DemoTab::TextInput
        | DemoTab::Layout
        | DemoTab::ModifierShowcase
        | DemoTab::FilePicker => "apps/desktop-demo/src/app.rs",
        DemoTab::Animations => "apps/desktop-demo/src/app/animations.rs",
        DemoTab::InteractiveAnim => "apps/desktop-demo/src/app/interactive_anim.rs",
        DemoTab::WebFetch => "apps/desktop-demo/src/app/web_fetch.rs",
        DemoTab::LazyList => "apps/desktop-demo/src/app/lazy_list.rs",
        DemoTab::Mineswapper2 => "apps/desktop-demo/src/app/mineswapper2.rs",
        DemoTab::RecompositionLab => "apps/desktop-demo/src/app/recomposition_lab.rs",
        DemoTab::HackerNews => "apps/desktop-demo/src/app/hacker_news.rs",
        DemoTab::Images => "apps/desktop-demo/src/app/images.rs",
        DemoTab::Text => "apps/desktop-demo/src/app/text_showcase.rs",
        DemoTab::Winamp => "apps/desktop-demo/src/app/winamp/mod.rs",
        DemoTab::Xkcd => "apps/desktop-demo/src/app/xkcd.rs",
        DemoTab::Shaders => "apps/desktop-demo/src/app/shaders.rs",
        DemoTab::ShaderRect => "apps/desktop-demo/src/app/shader_rect.rs",
        DemoTab::Liquid => "apps/desktop-demo/src/app/liquid_ui.rs",
        DemoTab::GlassFeed => "apps/desktop-demo/src/app/glass_feed.rs",
        DemoTab::MarkdownViewer => "apps/desktop-demo/src/app/markdown.rs",
        DemoTab::Rotary => "apps/desktop-demo/src/app/rotary.rs",
        DemoTab::Wear => "apps/desktop-demo/src/app/wear.rs",
    }
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
    Ready(Rc<Vec<String>>),
    Error(String),
}

#[allow(non_snake_case)]
#[composable]
pub(crate) fn SourceToggleButton(showing: MutableState<bool>, modifier: Modifier) {
    let label = if showing.get() {
        "Hide source"
    } else {
        "Show source"
    };
    Button(
        modifier.rounded_corners(12.0).padding(10.0),
        ButtonSpec::default(),
        move || showing.set(!showing.get()),
        move || {
            Text(label, Modifier::empty(), TextStyle::default());
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
                    .map(|body| body.lines().map(str::to_owned).collect::<Vec<_>>())
                    .map_err(|error| format!("could not fetch the source: {error}"))
            },
            move |result| match result {
                Ok(lines) => state.set(SourceState::Ready(Rc::new(lines))),
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
fn SourceLine(number: usize, text: String) {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(10.0)),
        move || {
            Text(
                format!("{number:>4}"),
                Modifier::empty(),
                TextStyle::default(),
            );
            Text(text.clone(), Modifier::empty(), TextStyle::default());
        },
    );
}
