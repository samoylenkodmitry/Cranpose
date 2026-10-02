use std::sync::OnceLock;

use cranpose::liquid::{LiquidTheme, LiquidThemeSpec, SchemeMode};
use cranpose_core::{remember, rememberMutableStateOf, with_key, LaunchedEffect, MutableState};
use cranpose_foundation::lazy::{rememberLazyListState, LazyListState};
use cranpose_services::local_uri_handler;
use cranpose_ui::{
    composable, text::FontWeight, Alignment, Box, BoxSpec, BoxWithConstraints,
    BoxWithConstraintsScope, Button, ButtonSpec, Column, ColumnSpec, GraphicsLayer,
    LinearArrangement, Modifier, Row, RowSpec, ScrollState, Spacer, Text, VerticalAlignment,
};

use super::markdown::{MarkdownAppearance, MarkdownDocument};

mod visuals;

use visuals::{caption_style, text_style, ACCENT, BORDER, INK, MUTED, PAPER};

const GUIDE: &str = include_str!("../../../../docs/guide.md");
const GUIDE_URL: &str = "https://github.com/samoylenkodmitry/Cranpose/blob/main/docs/guide.md";
const REPOSITORY_URL: &str = "https://github.com/samoylenkodmitry/Cranpose";
const WHEEL_STEP: f32 = 64.0;
const SIDEBAR_WIDTH: f32 = 292.0;
const COMPACT_BREAKPOINT: f32 = 780.0;

struct Chapter {
    title: &'static str,
    body: &'static str,
    number: String,
}

fn chapters() -> &'static [Chapter] {
    static CHAPTERS: OnceLock<Vec<Chapter>> = OnceLock::new();
    CHAPTERS.get_or_init(|| {
        GUIDE
            .split("\n## ")
            .skip(1)
            .filter_map(|section| section.split_once('\n'))
            .enumerate()
            .map(|(index, (title, body))| Chapter {
                title,
                body,
                number: format!("{:02}", index + 1),
            })
            .collect()
    })
}

#[composable]
fn DocAction(label: &'static str, modifier: Modifier, on_click: impl Fn() + 'static) {
    Button(
        modifier.rounded_corners(12.0).padding(10.0),
        ButtonSpec::default(),
        on_click,
        move || {
            Text(
                label,
                Modifier::empty(),
                text_style(13.0, INK, FontWeight::BOLD),
            );
        },
    );
}

#[composable]
fn RepositoryLink() {
    let uri_handler = local_uri_handler().current();
    Button(
        Modifier::empty()
            .fill_max_width()
            .rounded_corners(14.0)
            .padding(12.0)
            .semantics(|config| {
                config.content_description = Some("View on GitHub".to_string());
            }),
        ButtonSpec::default(),
        move || {
            if let Err(error) = uri_handler.open_uri(REPOSITORY_URL) {
                log::error!("Could not open the Cranpose repository: {error:#}");
            }
        },
        || {
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(10.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                || {
                    visuals::RepositoryIcon(Modifier::empty().size_points(21.0, 21.0));
                    Text(
                        "View on GitHub",
                        Modifier::empty().weight(1.0),
                        text_style(13.0, INK, FontWeight::BOLD),
                    );
                    Text(
                        "↗",
                        Modifier::empty(),
                        text_style(17.0, ACCENT, FontWeight::NORMAL),
                    );
                },
            );
        },
    );
}

#[composable]
fn Brand(compact: bool) {
    Column(
        Modifier::empty(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(5.0)),
        move || {
            Text("CRANPOSE", Modifier::empty(), caption_style(ACCENT));
            if !compact {
                Text(
                    "The guide.",
                    Modifier::empty().heading(),
                    text_style(27.0, INK, FontWeight::BOLD),
                );
                Text(
                    "0.9.0 · toward 1.0",
                    Modifier::empty(),
                    text_style(12.0, MUTED, FontWeight::NORMAL),
                );
            }
        },
    );
}

#[composable]
fn SectionCard(
    index: usize,
    selected: MutableState<usize>,
    menu_open: MutableState<bool>,
    scroll: ScrollState,
    viewport_height: f32,
    compact: bool,
) {
    let chapter = &chapters()[index];
    let active = selected.get() == index;
    let reduce_motion = cranpose_services::local_accessibility_options()
        .current()
        .reduce_motion;
    let modifier = Modifier::empty()
        .fill_max_width()
        .height(56.0)
        .graphics_layer(move || {
            if compact || reduce_motion {
                return GraphicsLayer::default();
            }
            let center = 12.0 + index as f32 * WHEEL_STEP + 28.0 - scroll.value();
            let distance = ((center - viewport_height * 0.5) / (viewport_height * 0.65).max(1.0))
                .clamp(-1.0, 1.0);
            GraphicsLayer {
                translation_x: 13.0 * distance * distance,
                rotation_z: distance * -5.5,
                scale: 1.0 - distance.abs() * 0.07,
                alpha: 1.0 - distance.abs() * 0.18,
                ..Default::default()
            }
        })
        .background(if active {
            visuals::SELECTED
        } else {
            visuals::CARD
        })
        .rounded_corners(15.0)
        .padding(12.0)
        .semantics(move |config| {
            config.selected = Some(active);
            config.content_description = Some(chapter.title.to_string());
        });
    Button(
        modifier,
        ButtonSpec::default(),
        move || {
            selected.set(index);
            menu_open.set(false);
        },
        move || {
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(11.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    Text(
                        chapter.number.as_str(),
                        Modifier::empty().width(19.0),
                        text_style(11.0, if active { ACCENT } else { MUTED }, FontWeight::BOLD),
                    );
                    Text(
                        chapter.title,
                        Modifier::empty().weight(1.0),
                        text_style(
                            13.0,
                            INK,
                            if active {
                                FontWeight::BOLD
                            } else {
                                FontWeight::NORMAL
                            },
                        ),
                    );
                    if active {
                        Text(
                            "•",
                            Modifier::empty(),
                            text_style(15.0, ACCENT, FontWeight::BOLD),
                        );
                    }
                },
            );
        },
    );
}

#[composable]
fn SectionWheel(
    selected: MutableState<usize>,
    menu_open: MutableState<bool>,
    scroll: ScrollState,
    modifier: Modifier,
    compact: bool,
) {
    BoxWithConstraints(
        modifier.clip_to_bounds().semantics(|config| {
            config.content_description = Some("Documentation sections".to_string());
        }),
        move |bounds| {
            let height = bounds.max_height().0;
            let selection = selected.get();
            LaunchedEffect(
                (selection, height.to_bits(), scroll.max_value().to_bits()),
                move |_| {
                    let center = selection as f32 * WHEEL_STEP + WHEEL_STEP * 0.5;
                    scroll.scroll_to((center - height * 0.5).max(0.0));
                },
            );
            Column(
                Modifier::empty()
                    .fill_max_size()
                    .vertical_scroll(scroll, false)
                    .padding(12.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    for index in 0..chapters().len() {
                        with_key(&index, || {
                            SectionCard(index, selected, menu_open, scroll, height, compact);
                        });
                    }
                },
            );
        },
    );
}

#[composable]
fn Sidebar(
    selected: MutableState<usize>,
    menu_open: MutableState<bool>,
    scroll: ScrollState,
    document: LazyListState,
    modifier: Modifier,
    compact: bool,
) {
    Box(
        modifier,
        BoxSpec::new().content_alignment(Alignment::CENTER_END),
        move || {
            visuals::GlassPane(Modifier::empty().fill_max_size());
            Column(
                Modifier::empty().fill_max_size().padding(18.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(14.0)),
                move || {
                    if !compact {
                        Box(Modifier::empty().padding(12.0), BoxSpec::default(), || {
                            Brand(false);
                        });
                    }
                    Text(
                        "SECTIONS",
                        Modifier::empty().padding(12.0),
                        caption_style(MUTED),
                    );
                    SectionWheel(
                        selected,
                        menu_open,
                        scroll,
                        Modifier::empty().fill_max_width().weight(1.0),
                        compact,
                    );
                    Text(
                        "Scroll to explore · select to read",
                        Modifier::empty().padding(10.0),
                        text_style(10.0, MUTED, FontWeight::NORMAL),
                    );
                    Spacer(
                        Modifier::empty()
                            .fill_max_width()
                            .height(1.0)
                            .background(BORDER),
                    );
                    RepositoryLink();
                },
            );
            if !compact {
                visuals::ElectricEdge(
                    document,
                    scroll,
                    Modifier::empty().width(28.0).fill_max_height(),
                );
            }
        },
    );
}

#[composable]
fn Reader(
    selected: MutableState<usize>,
    document: LazyListState,
    modifier: Modifier,
    compact: bool,
) {
    let index = selected.get();
    let Some(chapter) = chapters().get(index) else {
        return;
    };
    Box(
        modifier
            .background(BORDER)
            .rounded_corners(26.0)
            .padding(1.0),
        BoxSpec::default(),
        move || {
            BoxWithConstraints(
                Modifier::empty()
                    .fill_max_size()
                    .background(PAPER)
                    .rounded_corners(25.0),
                move |bounds| {
                    Box(
                        Modifier::empty().fill_max_size(),
                        BoxSpec::new().content_alignment(Alignment::TOP_CENTER),
                        move || {
                            Column(
                                Modifier::empty()
                                    .width(bounds.max_width().0.min(800.0))
                                    .fill_max_height()
                                    .padding(if compact { 16.0 } else { 32.0 }),
                                ColumnSpec::new().vertical_arrangement(
                                    LinearArrangement::SpacedBy(if compact { 16.0 } else { 22.0 }),
                                ),
                                move || {
                                    Row(
                                        Modifier::empty().fill_max_width(),
                                        RowSpec::new().vertical_alignment(
                                            VerticalAlignment::CenterVertically,
                                        ),
                                        move || {
                                            Text(
                                                "DOCUMENTATION",
                                                Modifier::empty().weight(1.0),
                                                caption_style(MUTED),
                                            );
                                            Text(
                                                format!(
                                                    "{} / {:02}",
                                                    chapter.number,
                                                    chapters().len()
                                                ),
                                                Modifier::empty(),
                                                caption_style(ACCENT),
                                            );
                                        },
                                    );
                                    Text(
                                        chapter.title,
                                        Modifier::empty().heading(),
                                        text_style(
                                            if compact { 30.0 } else { 40.0 },
                                            INK,
                                            FontWeight::BOLD,
                                        ),
                                    );
                                    Spacer(
                                        Modifier::empty()
                                            .width(44.0)
                                            .height(3.0)
                                            .background(ACCENT)
                                            .rounded_corners(1.5),
                                    );
                                    Box(
                                        Modifier::empty()
                                            .fill_max_width()
                                            .weight(1.0)
                                            .clip_to_bounds(),
                                        BoxSpec::default(),
                                        move || {
                                            with_key(&chapter.title, || {
                                                MarkdownDocument(
                                                    chapter.body,
                                                    GUIDE_URL,
                                                    document,
                                                    MarkdownAppearance::Reader,
                                                );
                                            });
                                        },
                                    );
                                    Spacer(
                                        Modifier::empty()
                                            .fill_max_width()
                                            .height(1.0)
                                            .background(BORDER),
                                    );
                                    Row(
                                        Modifier::empty().fill_max_width(),
                                        RowSpec::new().vertical_alignment(
                                            VerticalAlignment::CenterVertically,
                                        ),
                                        move || {
                                            if index > 0 {
                                                DocAction(
                                                    "← Previous",
                                                    Modifier::empty(),
                                                    move || selected.set(index - 1),
                                                );
                                            } else {
                                                Text(
                                                    "Works offline",
                                                    Modifier::empty(),
                                                    text_style(11.0, MUTED, FontWeight::NORMAL),
                                                );
                                            }
                                            Spacer(Modifier::empty().weight(1.0));
                                            if index + 1 < chapters().len() {
                                                DocAction(
                                                    "Next section →",
                                                    Modifier::empty().background(visuals::SELECTED),
                                                    move || selected.set(index + 1),
                                                );
                                            }
                                        },
                                    );
                                },
                            );
                        },
                    );
                },
            );
        },
    );
}

#[composable]
pub(super) fn DocumentationTab() {
    let selected = rememberMutableStateOf(|| 0usize);
    let menu_open = rememberMutableStateOf(|| false);
    let scroll = remember(|| ScrollState::new(0.0)).with(|state| *state);
    let document = rememberLazyListState();
    LaunchedEffect(selected.get(), move |_| document.scroll_to_item(0, 0.0));
    LiquidTheme(
        LiquidThemeSpec {
            scheme: SchemeMode::Dark,
            accent: ACCENT,
            ..Default::default()
        },
        move || {
            BoxWithConstraints(
                Modifier::empty()
                    .fill_max_size()
                    .rounded_corners(22.0)
                    .clip_to_bounds()
                    .semantics(|config| {
                        config.content_description = Some("Cranpose documentation".to_string());
                    }),
                move |bounds| {
                    let compact = bounds.max_width().0 < COMPACT_BREAKPOINT;
                    visuals::Backdrop();
                    if compact {
                        Column(
                            Modifier::empty().fill_max_size().padding(10.0),
                            ColumnSpec::new()
                                .vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
                            move || {
                                Row(
                                    Modifier::empty().fill_max_width().padding(6.0),
                                    RowSpec::new()
                                        .vertical_alignment(VerticalAlignment::CenterVertically),
                                    move || {
                                        Brand(true);
                                        Spacer(Modifier::empty().weight(1.0));
                                        DocAction(
                                            if menu_open.get() {
                                                "Close sections"
                                            } else {
                                                "Sections"
                                            },
                                            Modifier::empty().background(visuals::SELECTED),
                                            move || menu_open.update(|open| *open = !*open),
                                        );
                                    },
                                );
                                if menu_open.get() {
                                    Sidebar(
                                        selected,
                                        menu_open,
                                        scroll,
                                        document,
                                        Modifier::empty().fill_max_width().weight(1.0),
                                        true,
                                    );
                                } else {
                                    Reader(
                                        selected,
                                        document,
                                        Modifier::empty().fill_max_width().weight(1.0),
                                        true,
                                    );
                                }
                            },
                        );
                    } else {
                        Row(
                            Modifier::empty().fill_max_size().padding(20.0),
                            RowSpec::new()
                                .horizontal_arrangement(LinearArrangement::SpacedBy(20.0)),
                            move || {
                                Sidebar(
                                    selected,
                                    menu_open,
                                    scroll,
                                    document,
                                    Modifier::empty().width(SIDEBAR_WIDTH).fill_max_height(),
                                    false,
                                );
                                Reader(
                                    selected,
                                    document,
                                    Modifier::empty().weight(1.0).fill_max_height(),
                                    false,
                                );
                            },
                        );
                    }
                },
            );
        },
    );
}
