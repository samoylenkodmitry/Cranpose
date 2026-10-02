use std::sync::OnceLock;

use cranpose::liquid::{LiquidTheme, LiquidThemeSpec, SchemeMode};
use cranpose_core::{mutableStateOf, remember, rememberMutableStateOf, with_key, MutableState};
use cranpose_foundation::lazy::{LazyListScope, LazyListState};
use cranpose_services::local_uri_handler;
use cranpose_ui::{
    composable, rememberDraggableState, text::FontWeight, Alignment, Box, BoxSpec, Button,
    ButtonSpec, Column, ColumnSpec, GraphicsLayer, LazyColumn, LazyColumnSpec, LinearArrangement,
    Modifier, Row, RowSpec, Spacer, Text, VerticalAlignment,
};
use cranpose_ui_layout::Axis;

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
fn Brand() {
    Column(
        Modifier::empty(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text("CRANPOSE", Modifier::empty(), caption_style(ACCENT));
            Text(
                "The guide.",
                Modifier::empty().heading(),
                text_style(30.0, INK, FontWeight::BOLD),
            );
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
    reader_open: MutableState<bool>,
    browse_position: MutableState<f32>,
    page: LazyListState,
}

impl DocumentationState {
    fn selected(self) -> usize {
        self.page
            .first_visible_item_index()
            .saturating_sub(1)
            .min(chapters().len() - 1)
    }

    fn position(self, wheel_only: bool) -> f32 {
        if wheel_only {
            return self.browse_position.get();
        }
        let item = self.page.first_visible_item_index();
        if item == 0 {
            return 0.0;
        }
        let offset = self.page.first_visible_item_scroll_offset();
        let fraction = self
            .page
            .get_cached_size(item)
            .map_or(0.0, |height| offset / height.max(1.0));
        (item.saturating_sub(1) as f32 + fraction).clamp(0.0, (chapters().len() - 1) as f32)
    }

    fn select(self, index: usize) {
        self.reader_open.set(true);
        self.page.scroll_to_item(index + 1, 0.0);
    }

    fn rotate(self, delta: f32, wheel_only: bool) {
        let position =
            (self.position(wheel_only) - delta / 120.0).clamp(0.0, (chapters().len() - 1) as f32);
        self.browse_position.set(position);
        let item = position.floor() as usize + 1;
        let offset = self
            .page
            .get_cached_size(item)
            .map_or(0.0, |height| position.fract() * height);
        self.page.scroll_to_item(item, offset);
    }
}

#[composable]
fn ChapterButton(index: usize, state: DocumentationState, modifier: Modifier) {
    let chapter = &chapters()[index];
    let active = state.selected() == index;
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
fn SectionWheel(state: DocumentationState, geometry: WheelGeometry, height: f32, compact: bool) {
    let drag = rememberDraggableState(move |delta| state.rotate(delta, compact));
    let position = state.position(compact);
    let entry_height = 58.0
        * cranpose_services::local_accessibility_options()
            .current()
            .font_scale
            .max(1.0);
    let exposed_width = if compact {
        geometry.width
    } else {
        geometry.reader_left()
    };
    Box(
        Modifier::empty()
            .width(exposed_width)
            .height((height - 250.0).max(1.0))
            .offset(0.0, 190.0)
            .clip_to_bounds()
            .content_description("Documentation wheel")
            .draggable(Axis::Vertical, drag),
        BoxSpec::default(),
        move || {
            for index in 0..chapters().len() {
                with_key(&index, || {
                    let angle = (index as f32 - position) * geometry.section_angle();
                    let center = geometry.point(angle, geometry.radius + 120.0);
                    let modifier = Modifier::empty()
                        .size_points(246.0, entry_height)
                        .offset(center.x - 123.0, center.y - 190.0 - entry_height * 0.5)
                        .graphics_layer_value(GraphicsLayer {
                            rotation_z: -angle.to_degrees(),
                            ..Default::default()
                        });
                    ChapterButton(index, state, modifier);
                });
            }
        },
    );
}

#[composable]
fn ChapterNavigation(state: DocumentationState, index: usize) {
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
fn ReaderChapter(state: DocumentationState, index: usize, width: f32, height: f32, compact: bool) {
    let chapter = &chapters()[index];
    let inset = if compact { 20.0 } else { 40.0 };
    Box(
        visuals::reader_surface(
            Modifier::empty()
                .width(width)
                .height_in(height, f32::INFINITY),
        )
        .content_description("Documentation glass"),
        BoxSpec::new().content_alignment(Alignment::TOP_CENTER),
        move || {
            Column(
                Modifier::empty()
                    .width(width.min(840.0))
                    .padding_each(inset, 34.0, inset, 76.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(24.0)),
                move || {
                    Row(
                        Modifier::empty().fill_max_width(),
                        RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
                        move || {
                            Text(
                                if compact { "GUIDE" } else { "DOCUMENTATION" },
                                Modifier::empty().weight(1.0),
                                caption_style(MUTED),
                            );
                            Text(
                                format!("{} / {:02}", chapter.number, chapters().len()),
                                Modifier::empty(),
                                caption_style(ACCENT),
                            );
                        },
                    );
                    Text(
                        chapter.title,
                        Modifier::empty().heading(),
                        text_style(if compact { 34.0 } else { 46.0 }, INK, FontWeight::BOLD),
                    );
                    Spacer(Modifier::empty().width(44.0).height(3.0).background(ACCENT));
                    ChapterNavigation(state, index);
                    MarkdownDocument(chapter.body, GUIDE_URL);
                    Spacer(
                        Modifier::empty()
                            .fill_max_width()
                            .height(1.0)
                            .background(BORDER),
                    );
                    DocAction("Back to top", Modifier::empty(), move || {
                        state.page.scroll_to_item(0, 0.0);
                    });
                },
            );
        },
    );
}

#[composable]
fn GuideHeader(header: Option<super::AppHeaderState>) {
    if let Some(header) = header {
        super::AppHeader(
            header.active_tab,
            header.picker_open,
            header.showing_source,
            header.is_compact,
        );
    }
}

#[composable]
fn ScrollingHeader(
    header: Option<super::AppHeaderState>,
    state: DocumentationState,
    size: MutableState<cranpose_ui::Size>,
    reader_visible: bool,
) {
    let header_height = size.get().height;
    Box(
        Modifier::empty()
            .fill_max_width()
            .report_size_state(size)
            .graphics_layer(move || {
                let translation_y = if !reader_visible {
                    0.0
                } else if state.page.first_visible_item_index() == 0 {
                    -state.page.first_visible_item_scroll_offset()
                } else {
                    -header_height
                };
                GraphicsLayer {
                    translation_y,
                    ..Default::default()
                }
            }),
        BoxSpec::default(),
        move || GuideHeader(header),
    );
}

#[composable]
fn Reader(state: DocumentationState, width: f32, height: f32, compact: bool, header_height: f32) {
    let reader_left = if compact {
        0.0
    } else {
        WheelGeometry::new(width).reader_left()
    };
    let mut spec = LazyColumnSpec::new();
    spec.beyond_bounds_item_count = 0;
    LazyColumn(
        Modifier::empty().fill_max_size().padding_each(
            0.0,
            if compact { 48.0 } else { 0.0 },
            0.0,
            0.0,
        ),
        state.page,
        spec,
        move |scope| {
            scope.item_keyed(Some(0), Some(0), move || {
                Spacer(Modifier::empty().height(header_height));
            });
            scope.items(chapters().len(), move |index| {
                Row(
                    Modifier::empty().fill_max_width(),
                    RowSpec::default(),
                    move || {
                        Spacer(Modifier::empty().width(reader_left));
                        ReaderChapter(state, index, width - reader_left, height, compact);
                    },
                );
            });
        },
    );
}

#[composable]
pub(super) fn DocumentationTab(header: Option<super::AppHeaderState>) {
    let viewport = rememberMutableStateOf(cranpose_ui::Size::default);
    let header_size = rememberMutableStateOf(cranpose_ui::Size::default);
    let state = remember(|| DocumentationState {
        reader_open: mutableStateOf(false),
        browse_position: mutableStateOf(0.0),
        page: LazyListState::new(0, 0.0),
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
                    let reader_visible = !compact || state.reader_open.get();
                    visuals::Backdrop();
                    visuals::WheelSurface(geometry, state, !reader_visible);
                    if !compact || !reader_visible {
                        Box(
                            Modifier::empty().offset(28.0, 110.0),
                            BoxSpec::default(),
                            Brand,
                        );
                    }
                    let header_height = header_size.get().height;
                    if reader_visible && (header.is_none() || header_height > 0.0) {
                        Reader(state, width, size.height, compact, header_height);
                    }
                    ScrollingHeader(header, state, header_size, reader_visible);
                    if !compact || !reader_visible {
                        SectionWheel(state, geometry, size.height, compact);
                    }
                    if compact && reader_visible {
                        Row(
                            visuals::reader_surface(
                                Modifier::empty().fill_max_width().height(48.0),
                            ),
                            RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
                            move || {
                                DocAction("Back to wheel", Modifier::empty(), move || {
                                    state.browse_position.set(state.position(false));
                                    state.reader_open.set(false);
                                });
                                Spacer(Modifier::empty().weight(1.0));
                                Text(
                                    chapters()[state.selected()].number.as_str(),
                                    Modifier::empty().padding(12.0),
                                    caption_style(ACCENT),
                                );
                            },
                        );
                    }
                    Box(
                        Modifier::empty().fill_max_size(),
                        BoxSpec::new().content_alignment(Alignment::BOTTOM_START),
                        || {
                            Box(
                                visuals::reader_surface(Modifier::empty()),
                                BoxSpec::default(),
                                RepositoryLink,
                            );
                        },
                    );
                },
            );
        },
    );
}
