use std::sync::OnceLock;

use cranpose::liquid::{LiquidTheme, LiquidThemeSpec, SchemeMode};
use cranpose_core::{
    mutableStateOf, remember, rememberMutableStateOf, with_key, LaunchedEffect, MutableState,
};
use cranpose_services::local_uri_handler;
use cranpose_ui::{
    composable, text::FontWeight, Alignment, Box, BoxSpec, Button, ButtonSpec, Column, ColumnSpec,
    GraphicsLayer, LinearArrangement, Modifier, Row, RowSpec, ScrollState, Spacer, Text,
    VerticalAlignment,
};

use super::markdown::MarkdownDocument;
mod visuals;
use visuals::{caption_style, text_style, WheelGeometry, ACCENT, BORDER, INK, MUTED};

const GUIDE: &str = include_str!("../../../../docs/guide.md");
const GUIDE_URL: &str = "https://github.com/samoylenkodmitry/Cranpose/blob/main/docs/guide.md";
const REPOSITORY_URL: &str = "https://github.com/samoylenkodmitry/Cranpose";
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
        modifier.padding(12.0),
        ButtonSpec::default(),
        on_click,
        move || {
            Text(
                label,
                Modifier::empty(),
                text_style(12.0, INK, FontWeight::BOLD),
            );
        },
    );
}

#[composable]
fn RepositoryLink() {
    let uri_handler = local_uri_handler().current();
    Button(
        Modifier::empty()
            .padding(10.0)
            .content_description("View on GitHub"),
        ButtonSpec::default(),
        move || {
            if let Err(error) = uri_handler.open_uri(REPOSITORY_URL) {
                log::error!("Could not open the Cranpose repository: {error:#}");
            }
        },
        || {
            Row(
                Modifier::empty(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(10.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                || {
                    visuals::RepositoryIcon(Modifier::empty().size_points(21.0, 21.0));
                    Text(
                        "View on GitHub",
                        Modifier::empty(),
                        text_style(12.0, INK, FontWeight::BOLD),
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
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text("CRANPOSE", Modifier::empty(), caption_style(ACCENT));
            if !compact {
                Text(
                    "The guide.",
                    Modifier::empty().heading(),
                    text_style(30.0, INK, FontWeight::BOLD),
                );
            }
            Text(
                "0.9.0 · toward 1.0",
                Modifier::empty(),
                text_style(12.0, MUTED, FontWeight::NORMAL),
            );
        },
    );
}

#[derive(Clone, Copy, PartialEq)]
struct DocumentationState {
    selected: MutableState<usize>,
    menu_open: MutableState<bool>,
    page: ScrollState,
}

impl DocumentationState {
    fn select(self, index: usize) {
        self.selected.set(index);
        self.menu_open.set(false);
        self.page.scroll_to(0.0);
    }
}

#[composable]
fn ChapterButton(index: usize, state: DocumentationState, modifier: Modifier) {
    let chapter = &chapters()[index];
    let active = state.selected.get() == index;
    Button(
        modifier
            .padding_each(12.0, 14.0, 12.0, 14.0)
            .semantics(move |config| {
                config.selected = Some(active);
                config.content_description = Some(chapter.title.to_string());
            }),
        ButtonSpec::default(),
        move || state.select(index),
        move || {
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    Text(
                        chapter.number.as_str(),
                        Modifier::empty().width(22.0),
                        text_style(11.0, if active { ACCENT } else { MUTED }, FontWeight::BOLD),
                    );
                    Text(
                        chapter.title,
                        Modifier::empty().weight(1.0),
                        text_style(
                            14.0,
                            INK,
                            if active {
                                FontWeight::BOLD
                            } else {
                                FontWeight::NORMAL
                            },
                        ),
                    );
                },
            );
        },
    );
}

#[composable]
fn SectionWheel(state: DocumentationState, geometry: WheelGeometry) {
    let selected = state.selected.get();
    let reduce_motion = cranpose_services::local_accessibility_options()
        .current()
        .reduce_motion;
    Box(
        Modifier::empty()
            .size_points(geometry.width, geometry.height())
            .content_description("Documentation wheel"),
        BoxSpec::default(),
        move || {
            visuals::WheelSurface(geometry, state.page);
            Box(
                Modifier::empty().offset(28.0, 34.0),
                BoxSpec::default(),
                || Brand(false),
            );
            Box(
                Modifier::empty()
                    .width(geometry.reader_left())
                    .height(geometry.height() - 180.0)
                    .offset(0.0, 180.0)
                    .clip_to_bounds(),
                BoxSpec::default(),
                move || {
                    for index in 0..chapters().len() {
                        with_key(&index, || {
                            let resting_angle = (index as f32 - selected as f32) * 0.145;
                            let resting_center =
                                geometry.point(resting_angle, geometry.radius + 120.0);
                            let modifier = Modifier::empty()
                                .size_points(246.0, 58.0)
                                .offset(resting_center.x - 123.0, resting_center.y - 209.0)
                                .graphics_layer(move || {
                                    let travel = if reduce_motion {
                                        0.0
                                    } else {
                                        state.page.value() * 0.00012
                                    };
                                    let angle = resting_angle - travel;
                                    let center = geometry.point(angle, geometry.radius + 120.0);
                                    GraphicsLayer {
                                        translation_x: center.x - resting_center.x,
                                        translation_y: center.y - resting_center.y,
                                        rotation_z: -angle.to_degrees(),
                                        ..Default::default()
                                    }
                                });
                            ChapterButton(index, state, modifier);
                        });
                    }
                },
            );
        },
    );
}

#[composable]
fn ChapterNavigation(state: DocumentationState) {
    let index = state.selected.get();
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            if index > 0 {
                DocAction("Previous section", Modifier::empty(), move || {
                    state.select(index - 1);
                });
            } else {
                Text(
                    "Works offline",
                    Modifier::empty().padding(12.0),
                    text_style(11.0, MUTED, FontWeight::NORMAL),
                );
            }
            Spacer(Modifier::empty().weight(1.0));
            if index + 1 < chapters().len() {
                DocAction("Next section", Modifier::empty(), move || {
                    state.select(index + 1);
                });
            }
        },
    );
}

#[composable]
fn Reader(state: DocumentationState, width: f32, min_height: f32, compact: bool) {
    let chapter = &chapters()[state.selected.get()];
    let inset = if compact { 20.0 } else { 40.0 };
    Box(
        visuals::reader_surface(
            Modifier::empty()
                .width(width)
                .height_in(min_height, f32::INFINITY),
        )
        .content_description("Documentation glass"),
        BoxSpec::new().content_alignment(Alignment::TOP_CENTER),
        move || {
            Column(
                Modifier::empty()
                    .width(width.min(840.0))
                    .padding_each(inset, 34.0, inset, 40.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(24.0)),
                move || {
                    Row(
                        Modifier::empty().fill_max_width(),
                        RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
                        move || {
                            Text(
                                "DOCUMENTATION",
                                Modifier::empty().weight(1.0),
                                caption_style(MUTED),
                            );
                            Text(
                                format!("{} / {:02}", chapter.number, chapters().len()),
                                Modifier::empty(),
                                caption_style(ACCENT),
                            );
                            DocAction(
                                if state.menu_open.get() {
                                    "Close sections"
                                } else {
                                    "Sections"
                                },
                                Modifier::empty(),
                                move || {
                                    state.menu_open.update(|open| *open = !*open);
                                    state.page.scroll_to(0.0);
                                },
                            );
                        },
                    );
                    if compact {
                        Brand(true);
                    }
                    if state.menu_open.get() {
                        Text(
                            "Explore the guide",
                            Modifier::empty().heading(),
                            text_style(30.0, INK, FontWeight::BOLD),
                        );
                        for index in 0..chapters().len() {
                            with_key(&index, || {
                                ChapterButton(index, state, Modifier::empty().fill_max_width());
                            });
                        }
                    } else {
                        Text(
                            chapter.title,
                            Modifier::empty().heading(),
                            text_style(if compact { 34.0 } else { 46.0 }, INK, FontWeight::BOLD),
                        );
                        Spacer(Modifier::empty().width(44.0).height(3.0).background(ACCENT));
                        ChapterNavigation(state);
                        MarkdownDocument(chapter.body, GUIDE_URL);
                        Spacer(
                            Modifier::empty()
                                .fill_max_width()
                                .height(1.0)
                                .background(BORDER),
                        );
                        DocAction("Back to top", Modifier::empty(), move || {
                            state.page.scroll_to(0.0);
                        });
                    }
                    RepositoryLink();
                },
            );
        },
    );
}

#[composable]
pub(super) fn DocumentationTab(header: Option<super::AppHeaderState>) {
    let viewport = rememberMutableStateOf(cranpose_ui::Size::default);
    let state = remember(|| DocumentationState {
        selected: mutableStateOf(0usize),
        menu_open: mutableStateOf(false),
        page: ScrollState::new(0.0),
    })
    .with(|state| *state);
    LiquidTheme(
        LiquidThemeSpec {
            scheme: SchemeMode::Dark,
            accent: ACCENT,
            ..Default::default()
        },
        move || {
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .report_size_state(viewport)
                    .clip_to_bounds()
                    .pane_title("Documentation")
                    .content_description("Cranpose documentation"),
                BoxSpec::default(),
                move || {
                    let size = viewport.get();
                    let width = size.width.max(1.0);
                    let compact = width < COMPACT_BREAKPOINT;
                    let geometry = WheelGeometry::new(width);
                    let reader_left = if compact { 0.0 } else { geometry.reader_left() };
                    let min_height = geometry.height().max(size.height);
                    let menu_open = state.menu_open.get();
                    let selection = state.selected.get();
                    let viewport_height = size.height;
                    let font_scale = cranpose_services::local_accessibility_options()
                        .current()
                        .font_scale;
                    LaunchedEffect((menu_open, selection), move |_| {
                        if menu_open {
                            state.page.scroll_to(
                                (240.0 + selection as f32 * 64.0 * font_scale.max(1.0)
                                    - viewport_height * 0.5)
                                    .max(0.0),
                            );
                        }
                    });
                    visuals::Backdrop();
                    Column(
                        Modifier::empty()
                            .fill_max_size()
                            .vertical_scroll(state.page, false),
                        ColumnSpec::default(),
                        move || {
                            if let Some(header) = header {
                                super::AppHeader(
                                    header.active_tab,
                                    header.picker_open,
                                    header.showing_source,
                                    header.is_compact,
                                );
                            }
                            Box(
                                Modifier::empty().fill_max_width().clip_to_bounds(),
                                BoxSpec::default(),
                                move || {
                                    if !compact {
                                        SectionWheel(state, geometry);
                                    } else {
                                        visuals::WheelSurface(geometry, state.page);
                                    }
                                    Row(
                                        Modifier::empty().fill_max_width(),
                                        RowSpec::default(),
                                        move || {
                                            Spacer(Modifier::empty().width(reader_left));
                                            Reader(state, width - reader_left, min_height, compact);
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
