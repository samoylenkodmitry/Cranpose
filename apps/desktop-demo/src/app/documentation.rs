use std::sync::OnceLock;

use cranpose::liquid::{LiquidTheme, LiquidThemeSpec, SchemeMode};
use cranpose_animation::{animate_float_as_state_with_initial, tween, Easing};
use cranpose_core::{
    mutableStateOf, remember, rememberMutableStateOf, with_key, MutableState, SideEffect,
};
use cranpose_foundation::lazy::{LazyListScope, LazyListState};
use cranpose_services::local_uri_handler;
use cranpose_ui::{
    composable, rememberScrollableState, text::FontWeight, Alignment, Box, BoxSpec, Button,
    ButtonSpec, Column, ColumnSpec, LazyColumn, LazyColumnSpec, LinearArrangement, Modifier,
    PointerEventKind, PointerInputScope, Row, RowSpec, Spacer, Text, VerticalAlignment,
};
use cranpose_ui_layout::Axis;

use super::markdown::{render_markdown_block, MarkdownAppearance};
mod document;
use document::{ReaderItem, DOCUMENT};
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
                "Build apps.",
                Modifier::empty().heading(),
                text_style(30.0, INK, FontWeight::BOLD),
            );
            Text(
                "Compose in Rust",
                Modifier::empty(),
                text_style(12.0, MUTED, FontWeight::NORMAL),
            );
        },
    );
}

#[derive(Clone, Copy, PartialEq)]
struct SelectionMotion {
    from: f32,
    target: f32,
    generation: u64,
}

#[derive(Clone, Copy, PartialEq)]
struct DocumentationState {
    reader_open: MutableState<bool>,
    browse_position: MutableState<f32>,
    selection: MutableState<Option<SelectionMotion>>,
    page: LazyListState,
}

impl DocumentationState {
    fn selected(self) -> usize {
        DOCUMENT.with(|document| document.chapter(self.page.first_visible_item_index()))
    }

    fn position(self, wheel_only: bool) -> f32 {
        if wheel_only {
            return self.browse_position.get();
        }
        self.reader_position()
            .clamp(0.0, (chapters().len() - 1) as f32)
    }

    fn reader_position(self) -> f32 {
        let item = self.page.first_visible_item_index();
        let offset = self.page.first_visible_item_scroll_offset();
        if item == 0 {
            return -1.0 + offset / self.page.get_cached_size(0).unwrap_or(1.0).max(1.0);
        }
        let fraction = self
            .page
            .get_cached_size(item)
            .map_or(0.0, |height| offset / height.max(1.0));
        DOCUMENT.with(|document| document.position(item, fraction))
    }

    fn header_position(self) -> f32 {
        -1.0
    }

    fn select(self, index: usize, wheel_only: bool) {
        self.animate_to(index as f32, wheel_only);
        self.reader_open.set(true);
    }

    fn animate_to(self, target: f32, wheel_only: bool) {
        self.selection.set(Some(SelectionMotion {
            from: if wheel_only {
                self.browse_position.get()
            } else {
                self.reader_position()
            },
            target,
            generation: self
                .selection
                .get()
                .map_or(0, |motion| motion.generation.wrapping_add(1)),
        }));
    }

    fn scroll_to_position(self, position: f32) {
        if position < 0.0 {
            let offset = (position + 1.0) * self.page.get_cached_size(0).unwrap_or(0.0);
            self.page.scroll_to_item(0, offset.max(0.0));
            return;
        }
        let (item, fraction) = DOCUMENT.with(|document| document.item(position));
        let offset = self.page.get_cached_size(item).unwrap_or(0.0) * fraction;
        self.page.scroll_to_item(item, offset);
    }

    fn rotate(self, delta: f32, wheel_only: bool) -> f32 {
        self.selection.set(None);
        let previous = self.position(wheel_only);
        let position = (previous - delta / 120.0).clamp(0.0, (chapters().len() - 1) as f32);
        self.browse_position.set(position);
        self.scroll_to_position(position);
        (previous - position) * 120.0
    }
}

#[composable]
fn AnimateSelection(state: DocumentationState) {
    if let Some(motion) = state.selection.get() {
        with_key(&motion.generation, || {
            let reduced = cranpose_services::local_accessibility_options()
                .current()
                .reduce_motion;
            let position = if reduced {
                motion.target
            } else {
                animate_float_as_state_with_initial(
                    motion.from,
                    motion.target,
                    tween(450, Easing::EaseInOut),
                    "documentation chapter",
                )
                .get()
            };
            SideEffect(move || {
                state.scroll_to_position(position);
                if (position - motion.target).abs() < f32::EPSILON {
                    state.selection.set(None);
                }
            });
        });
    }
}

#[composable]
fn ChapterLabel(index: usize, active: bool) {
    let chapter = &chapters()[index];
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding_each(12.0, 14.0, 12.0, 14.0),
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
}

#[composable]
fn WheelEntries(
    state: DocumentationState,
    geometry: WheelGeometry,
    wheel_only: bool,
    interactive: bool,
    first: usize,
    end: usize,
) {
    let position = state.position(wheel_only);
    let entry_height = 58.0
        * cranpose_services::local_accessibility_options()
            .current()
            .font_scale
            .max(1.0);
    for index in first..end {
        with_key(&index, || {
            let modifier = geometry.entry_modifier(index as f32 - position, entry_height);
            let active = state.selected() == index;
            if interactive {
                Button(
                    modifier.semantics(move |config| {
                        config.selected = Some(active);
                        config.content_description = Some(chapters()[index].title.to_string());
                    }),
                    ButtonSpec::default(),
                    move || state.select(index, wheel_only),
                    || {},
                );
            } else {
                Box(modifier, BoxSpec::default(), move || {
                    ChapterLabel(index, active);
                });
            }
        });
    }
}

#[composable]
fn WheelVisuals(state: DocumentationState, geometry: WheelGeometry, wheel_only: bool) {
    visuals::WheelSurface(geometry, move || state.position(wheel_only));
    if geometry.width < COMPACT_BREAKPOINT {
        Box(
            Modifier::empty().offset(12.0, 130.0).width(154.0),
            BoxSpec::default(),
            Brand,
        );
    } else {
        let position = state.position(wheel_only);
        let visible = geometry.visible_entries(position, chapters().len());
        Box(
            geometry.brand_modifier(-1.35 - position).padding(12.0),
            BoxSpec::default(),
            Brand,
        );
        Box(
            Modifier::empty().fill_max_size().hide_from_accessibility(),
            BoxSpec::default(),
            move || {
                WheelEntries(
                    state,
                    geometry,
                    wheel_only,
                    false,
                    visible.start,
                    visible.end,
                );
            },
        );
    }
}

#[composable]
fn MobileChapters(state: DocumentationState, width: f32, height: f32) {
    let list = remember(|| LazyListState::new(state.position(true).floor() as usize, 0.0))
        .with(|list| *list);
    let left = (width * 0.45).max(152.0).ceil();
    let mut spec = LazyColumnSpec::default();
    spec.beyond_bounds_item_count = 0;
    LazyColumn(
        Modifier::empty()
            .offset(left, 96.0)
            .width(width - left)
            .height((height - 156.0).max(44.0))
            .clip_to_bounds()
            .content_description("Guide chapters"),
        list,
        spec,
        move |scope| {
            scope.items(chapters().len(), move |index| {
                let active = state.position(true).round() as usize == index;
                Button(
                    Modifier::empty()
                        .fill_max_width()
                        .height_in(52.0, f32::INFINITY)
                        .semantics(move |config| {
                            config.selected = Some(active);
                            config.content_description = Some(chapters()[index].title.to_owned());
                        }),
                    ButtonSpec::default(),
                    move || state.select(index, true),
                    move || ChapterLabel(index, active),
                );
            });
        },
    );
}

#[composable]
fn SectionWheel(state: DocumentationState, geometry: WheelGeometry, height: f32, compact: bool) {
    let scroll = rememberScrollableState(move |delta| state.rotate(delta, compact));
    let visible = geometry.visible_entries(state.position(compact), chapters().len());
    Box(
        Modifier::empty()
            .width(if compact {
                geometry.width
            } else {
                geometry.reader_left()
            })
            .height((height - 44.0).max(1.0))
            .clip_to_bounds()
            .content_description("Guide wheel")
            .scrollable(Axis::Vertical, scroll),
        BoxSpec::default(),
        move || {
            if !compact {
                WheelEntries(state, geometry, compact, true, visible.start, visible.end);
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
                    state.select(index - 1, false);
                });
            }
            Spacer(Modifier::empty().weight(1.0));
            if index + 1 < chapters().len() {
                DocAction("Next section", Modifier::empty(), move || {
                    state.select(index + 1, false);
                });
            }
        },
    );
}

#[composable]
fn ChapterHeading(state: DocumentationState, index: usize, compact: bool) {
    let chapter = &chapters()[index];
    Column(
        Modifier::empty()
            .fill_max_width()
            .padding_each(0.0, 34.0, 0.0, 0.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(24.0)),
        move || {
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    Text(
                        if compact { "GUIDE" } else { "CRANPOSE GUIDE" },
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
        },
    );
}

#[composable]
fn ReaderBlock(state: DocumentationState, index: usize, width: f32, compact: bool) {
    DOCUMENT.with(|document| match &document.items[index] {
        ReaderItem::Heading(chapter) => ChapterHeading(state, *chapter, compact),
        ReaderItem::Block(block) => render_markdown_block(block, MarkdownAppearance::Reader, width),
        ReaderItem::Footer => {
            Column(
                Modifier::empty()
                    .fill_max_width()
                    .padding_each(0.0, 12.0, 0.0, 58.0),
                ColumnSpec::default(),
                move || {
                    Spacer(
                        Modifier::empty()
                            .fill_max_width()
                            .height(1.0)
                            .background(BORDER),
                    );
                    DocAction("Back to top", Modifier::empty(), move || {
                        state.animate_to(state.header_position(), false);
                    });
                },
            );
        }
    });
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
    let offset = if !reader_visible {
        -(state.position(true) * 120.0).min(header_height)
    } else if state.page.first_visible_item_index() == 0 {
        -state.page.first_visible_item_scroll_offset()
    } else {
        -header_height
    };
    Box(
        Modifier::empty()
            .fill_max_width()
            .report_size_state(size)
            .offset(0.0, offset),
        BoxSpec::default(),
        move || GuideHeader(header),
    );
}

#[composable]
fn Reader(state: DocumentationState, width: f32, height: f32, compact: bool, header_height: f32) {
    let reader_left = if compact {
        0.0
    } else {
        WheelGeometry::new(width, height).reader_left()
    };
    let inset = if compact { 20.0 } else { 24.0 };
    let content_width = (width - reader_left).min(840.0) - inset * 2.0;
    Box(
        visuals::reader_surface(
            Modifier::empty()
                .offset(reader_left, 0.0)
                .width(width - reader_left)
                .height(height),
        )
        .content_description("Guide glass"),
        BoxSpec::default(),
        || {},
    );
    let mut spec = LazyColumnSpec::new();
    spec.beyond_bounds_item_count = 0;
    LazyColumn(
        Modifier::empty()
            .fill_max_size()
            .pointer_input((), move |scope: PointerInputScope| async move {
                scope
                    .await_pointer_event_scope(|events| async move {
                        loop {
                            let event = events.await_pointer_event().await;
                            if matches!(
                                event.kind,
                                PointerEventKind::Down | PointerEventKind::Scroll
                            ) {
                                state.selection.set(None);
                            }
                        }
                    })
                    .await;
            })
            .padding_each(0.0, if compact { 48.0 } else { 0.0 }, 0.0, 0.0),
        state.page,
        spec,
        move |scope| {
            scope.item_keyed(Some(0), Some(0), move || {
                Spacer(Modifier::empty().height(header_height));
            });
            scope.items(
                DOCUMENT.with(|document| document.items.len()),
                move |index| {
                    Box(
                        Modifier::empty().fill_max_width().padding_each(
                            reader_left + inset,
                            0.0,
                            inset,
                            18.0,
                        ),
                        BoxSpec::default(),
                        move || {
                            Box(
                                Modifier::empty().width(content_width),
                                BoxSpec::default(),
                                move || {
                                    ReaderBlock(state, index, content_width, compact);
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
pub(super) fn GuideTab(header: Option<super::AppHeaderState>) {
    let viewport = rememberMutableStateOf(cranpose_ui::Size::default);
    let header_size = rememberMutableStateOf(cranpose_ui::Size::default);
    let state = remember(|| DocumentationState {
        reader_open: mutableStateOf(false),
        browse_position: mutableStateOf(0.0),
        selection: mutableStateOf(None),
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
                    .pane_title("Cranpose Guide")
                    .content_description("Cranpose guide"),
                BoxSpec::default(),
                move || {
                    AnimateSelection(state);
                    let size = viewport.get();
                    if size.width <= 0.0 || size.height <= 0.0 {
                        return;
                    }
                    let width = size.width.max(1.0);
                    let compact = width < COMPACT_BREAKPOINT;
                    let geometry = WheelGeometry::new(width, size.height);
                    let reader_visible = !compact || state.reader_open.get();
                    visuals::Backdrop(geometry);
                    WheelVisuals(state, geometry, !reader_visible);
                    let header_height = header_size.get().height;
                    if reader_visible && (header.is_none() || header_height > 0.0) {
                        Reader(state, width, size.height, compact, header_height);
                    }
                    if !compact || !reader_visible {
                        SectionWheel(state, geometry, size.height, compact);
                        if compact {
                            MobileChapters(state, width, size.height);
                        }
                    }
                    ScrollingHeader(header, state, header_size, reader_visible);
                    if compact && reader_visible {
                        Row(
                            visuals::reader_surface(
                                Modifier::empty().fill_max_width().height(48.0),
                            ),
                            RowSpec::new().vertical_alignment(VerticalAlignment::CenterVertically),
                            move || {
                                DocAction("Back to wheel", Modifier::empty(), move || {
                                    state.selection.set(None);
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
