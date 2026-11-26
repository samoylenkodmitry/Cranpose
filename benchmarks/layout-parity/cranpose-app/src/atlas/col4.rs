use std::rc::Rc;

use cranpose::{LazyItems, prelude::*};
use cranpose_foundation::lazy::{
    LazyListScope, rememberLazyListState, rememberLazyListStateWithPosition,
};
use cranpose_ui::{
    PaddingValues, ScrollState,
    widgets::{
        GridCells, HorizontalPager, LazyVerticalGrid, LazyVerticalGridSpec,
        LazyVerticalStaggeredGrid, LazyVerticalStaggeredGridSpec, PageSize, PagerSpec,
        SnapPosition, rememberLazyGridState, rememberLazyStaggeredGridState, rememberPagerState,
    },
};
use cranpose_ui_graphics::{Color, RoundedCornerShape};
use cranpose_ui_layout::{Alignment, LinearArrangement, VerticalAlignment};

use super::common::{
    ACCENTS, AMBER, AMBER_SOFT, BG_CARD, BG_SUBTLE, BLUE, BLUE_SOFT, BORDER_CARD, BORDER_STRONG,
    CYAN, ChipSpec, EMERALD, EMERALD_SOFT, FullWidthChip, INDIGO, INK, LabeledChip, RowChipSpec,
    TEAL, ZoneCard, atlas_text_style, rect_shape,
};

#[composable]
pub(super) fn AtlasCol4(modifier: Modifier) {
    Column(
        modifier.fill_max_height(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
        || {
            Zone13ScrollAndLazyRow(Modifier::empty().fill_max_width().columnWeight(0.88, true));
            Zone14StickyHeader(Modifier::empty().fill_max_width().columnWeight(1.06, true));
            Zone15GridsAndStaggered(Modifier::empty().fill_max_width().columnWeight(1.12, true));
            Zone16PagerAndDeepNest(Modifier::empty().fill_max_width().columnWeight(0.94, true));
        },
    );
}

#[composable]
fn Zone13ReversedLazyRow() {
    let lazy_row_state = rememberLazyListState();
    LazyRow(
        Modifier::empty()
            .test_tag("z13_lrow")
            .fill_max_width()
            .height(36.0)
            .background(BLUE_SOFT)
            .border(1.0, BLUE, rect_shape()),
        lazy_row_state,
        LazyRowSpec::new()
            .content_padding(PaddingValues::symmetric(10.0, 4.0))
            .reverse_layout(true)
            .horizontal_arrangement(LinearArrangement::spaced_by(5.0)),
        |scope| {
            scope.items(LazyItems::new(10).key(|i| i as u64), |i| {
                LabeledChip(
                    format!("z13_lr_{i}"),
                    format!("Rev {i}"),
                    ChipSpec {
                        width: 46.0,
                        height: 26.0,
                        bg: ACCENTS[i % ACCENTS.len()],
                        radius: 4.0,
                        font_size: 8.0,
                    },
                );
            });
        },
    );
}

#[composable]
fn Zone13ScrollAndLazyRow(modifier: Modifier) {
    let v_state = remember(|| ScrollState::new(22.0)).with(Clone::clone);
    let h_state = remember(|| ScrollState::new(34.0)).with(Clone::clone);

    ZoneCard(
        "z13_card",
        "13. verticalScroll · horizontalScroll · LazyRow(reverse)",
        CYAN,
        modifier,
        move || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(5.0)),
                move || {
                    Column(
                        Modifier::empty()
                            .test_tag("z13_vscroll")
                            .fill_max_width()
                            .height(50.0)
                            .background(BG_SUBTLE)
                            .border(1.0, BORDER_STRONG, rect_shape())
                            .vertical_scroll(v_state, false)
                            .padding(4.0),
                        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(4.0)),
                        || {
                            for i in 0..5 {
                                FullWidthChip(
                                    format!("z13_vs_{i}"),
                                    format!("VScroll Row {i}"),
                                    RowChipSpec {
                                        height: 18.0,
                                        bg: ACCENTS[i % ACCENTS.len()],
                                        radius: 3.0,
                                        font_size: 8.0,
                                        alignment: Alignment::CENTER_START,
                                        horizontal_padding: 6.0,
                                    },
                                );
                            }
                        },
                    );
                    Row(
                        Modifier::empty()
                            .test_tag("z13_hscroll")
                            .fill_max_width()
                            .height(26.0)
                            .background(BG_SUBTLE)
                            .border(1.0, BORDER_STRONG, rect_shape())
                            .horizontal_scroll(h_state, false)
                            .padding(3.0),
                        RowSpec::new()
                            .horizontal_arrangement(LinearArrangement::spaced_by(5.0))
                            .vertical_alignment(VerticalAlignment::CenterVertically),
                        || {
                            for i in 0..7 {
                                LabeledChip(
                                    format!("z13_hs_{i}"),
                                    format!("HS {i}"),
                                    ChipSpec {
                                        width: 56.0,
                                        height: 18.0,
                                        bg: ACCENTS[(i + 1) % ACCENTS.len()],
                                        radius: 3.0,
                                        font_size: 8.0,
                                    },
                                );
                            }
                        },
                    );
                    Zone13ReversedLazyRow();
                },
            );
        },
    );
}

#[composable]
fn StickyGroupItem(tag: String, label: String, bg: Color, stroke: Color) {
    Box(
        Modifier::empty()
            .test_tag(tag)
            .fill_max_width()
            .height(24.0)
            .background(bg)
            .rounded_corners(3.0)
            .border(1.0, stroke, RoundedCornerShape::uniform(3.0))
            .padding_horizontal(6.0),
        BoxSpec::new().content_alignment(Alignment::CENTER_START),
        move || {
            Text(
                label.clone(),
                Modifier::empty(),
                atlas_text_style(8.5, INK, false),
            );
        },
    );
}

#[composable]
fn Zone14StickyHeader(modifier: Modifier) {
    let state = rememberLazyListStateWithPosition(2, 10.0);

    ZoneCard(
        "z14_card",
        "14. LazyColumn · Asymmetric Padding · Pinned stickyHeader",
        INDIGO,
        modifier,
        move || {
            LazyColumn(
                Modifier::empty()
                    .test_tag("z14_lcol")
                    .fill_max_size()
                    .background(BG_SUBTLE)
                    .border(1.0, BORDER_STRONG, rect_shape()),
                state,
                LazyColumnSpec::new()
                    .content_padding(PaddingValues::new(10.0, 6.0, 16.0, 6.0))
                    .vertical_arrangement(LinearArrangement::spaced_by(4.0)),
                |scope| {
                    scope.sticky_header_keyed(Some(1000), None, || {
                        FullWidthChip(
                            "z14_hdr_a".to_owned(),
                            "SECTION A — PINNED HEADER".to_owned(),
                            RowChipSpec {
                                height: 22.0,
                                bg: BLUE,
                                radius: 3.0,
                                font_size: 8.5,
                                alignment: Alignment::CENTER_START,
                                horizontal_padding: 6.0,
                            },
                        );
                    });
                    scope.items(LazyItems::new(4).key(|i| 100 + i as u64), |i| {
                        StickyGroupItem(
                            format!("z14_a{i}"),
                            format!("Group A · Item {i}"),
                            BLUE_SOFT,
                            BLUE,
                        );
                    });
                    scope.sticky_header_keyed(Some(2000), None, || {
                        FullWidthChip(
                            "z14_hdr_b".to_owned(),
                            "SECTION B — APPROACHING".to_owned(),
                            RowChipSpec {
                                height: 22.0,
                                bg: EMERALD,
                                radius: 3.0,
                                font_size: 8.5,
                                alignment: Alignment::CENTER_START,
                                horizontal_padding: 6.0,
                            },
                        );
                    });
                    scope.items(LazyItems::new(6).key(|i| 200 + i as u64), |i| {
                        StickyGroupItem(
                            format!("z14_b{i}"),
                            format!("Group B · Item {i}"),
                            EMERALD_SOFT,
                            EMERALD,
                        );
                    });
                },
            );
        },
    );
}

#[composable]
fn Zone15FixedGrid() {
    let grid_state = rememberLazyGridState();
    let grid_heights: Rc<[f32]> = Rc::from([18.0, 18.0, 28.0, 22.0, 20.0, 20.0, 18.0, 18.0]);
    LazyVerticalGrid(
        Modifier::empty()
            .test_tag("z15_vgrid")
            .fill_max_width()
            .columnWeight(1.05, true)
            .background(BG_SUBTLE)
            .border(1.0, BORDER_STRONG, rect_shape()),
        grid_state,
        LazyVerticalGridSpec::new(GridCells::Fixed(3))
            .spacing(4.0)
            .content_padding(PaddingValues::all(4.0)),
        move |scope| {
            let gh = Rc::clone(&grid_heights);
            scope.items(
                LazyItems::new(gh.len())
                    .key(|idx| idx as u64)
                    .span(|span_scope, idx| match idx {
                        0 => span_scope.max_line_span,
                        4 | 6 | 7 => 2,
                        _ => 1,
                    }),
                move |idx| {
                    let label = match idx {
                        0 => "FullSpan (3)".to_owned(),
                        4 | 6 | 7 => format!("Span2 #{idx}"),
                        _ => format!("G{idx}"),
                    };
                    FullWidthChip(
                        format!("z15_vg_{idx}"),
                        label,
                        RowChipSpec {
                            height: gh[idx],
                            bg: ACCENTS[idx % ACCENTS.len()],
                            radius: 3.0,
                            font_size: 7.5,
                            alignment: Alignment::CENTER,
                            horizontal_padding: 0.0,
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn Zone15StaggeredGrid() {
    let staggered_state = rememberLazyStaggeredGridState();
    let stag_heights: Rc<[f32]> = Rc::from([22.0, 36.0, 18.0, 26.0, 16.0, 24.0, 28.0, 20.0]);
    LazyVerticalStaggeredGrid(
        Modifier::empty()
            .test_tag("z15_sgrid")
            .fill_max_width()
            .columnWeight(1.0, true)
            .background(AMBER_SOFT)
            .border(1.0, AMBER, rect_shape()),
        staggered_state,
        LazyVerticalStaggeredGridSpec::new(GridCells::Fixed(3))
            .spacing(4.0)
            .content_padding(PaddingValues::all(4.0)),
        move |scope| {
            let sh = Rc::clone(&stag_heights);
            scope.items(
                LazyItems::new(sh.len())
                    .key(|idx| idx as u64)
                    .span(|span_scope, idx| {
                        if idx == 4 {
                            span_scope.max_line_span
                        } else {
                            1
                        }
                    }),
                move |idx| {
                    let bg = if idx == 4 {
                        INK
                    } else {
                        ACCENTS[(idx + 3) % ACCENTS.len()]
                    };
                    let label = if idx == 4 {
                        "FullLine Barrier".to_owned()
                    } else {
                        format!("M{idx}")
                    };
                    FullWidthChip(
                        format!("z15_sg_{idx}"),
                        label,
                        RowChipSpec {
                            height: sh[idx],
                            bg,
                            radius: 3.0,
                            font_size: 7.5,
                            alignment: Alignment::CENTER,
                            horizontal_padding: 0.0,
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn Zone15GridsAndStaggered(modifier: Modifier) {
    ZoneCard(
        "z15_card",
        "15. LazyVerticalGrid (Spans/Wrap) · StaggeredGrid",
        AMBER,
        modifier,
        || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
                || {
                    Zone15FixedGrid();
                    Zone15StaggeredGrid();
                },
            );
        },
    );
}

#[composable]
fn Zone16PagerAndDeepNest(modifier: Modifier) {
    let pager_state = rememberPagerState(1);

    ZoneCard(
        "z16_card",
        "16. HorizontalPager (Fixed/Center) · 10-Level Nest",
        TEAL,
        modifier,
        move || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
                move || {
                    HorizontalPager(
                        Modifier::empty()
                            .test_tag("z16_pager")
                            .fill_max_width()
                            .height(60.0)
                            .background(BG_SUBTLE)
                            .border(1.0, BORDER_STRONG, rect_shape()),
                        pager_state,
                        PagerSpec {
                            page_size: PageSize::Fixed(128.0),
                            page_spacing: 8.0,
                            content_padding: PaddingValues::symmetric(16.0, 6.0),
                            snap_position: SnapPosition::Center,
                            ..PagerSpec::default()
                        },
                        5,
                        |page| {
                            Box(
                                Modifier::empty()
                                    .test_tag(format!("z16_pg_{page}"))
                                    .fill_max_size()
                                    .background(ACCENTS[page % ACCENTS.len()])
                                    .rounded_corners(6.0)
                                    .border(1.0, Color::WHITE, RoundedCornerShape::uniform(6.0)),
                                BoxSpec::new().content_alignment(Alignment::CENTER),
                                move || {
                                    Text(
                                        format!("Page {page}"),
                                        Modifier::empty(),
                                        atlas_text_style(9.5, Color::WHITE, true),
                                    );
                                },
                            );
                        },
                    );
                    DeepNestedChain(
                        0,
                        10,
                        Modifier::empty().fill_max_width().columnWeight(1.0, true),
                    );
                },
            );
        },
    );
}

#[composable]
fn DeepNestedChain(level: usize, max_depth: usize, modifier: Modifier) {
    if level >= max_depth {
        Box(
            modifier
                .test_tag("z16_deep_core")
                .fill_max_size()
                .background(BLUE),
            BoxSpec::new().content_alignment(Alignment::CENTER),
            || {
                Text(
                    "Depth 10 Core",
                    Modifier::empty(),
                    atlas_text_style(8.0, Color::WHITE, true),
                );
            },
        );
        return;
    }
    let bg = if level.is_multiple_of(2) {
        BLUE_SOFT
    } else {
        BG_CARD
    };
    let m = modifier
        .test_tag(format!("z16_deep_{level}"))
        .fill_max_size()
        .background(bg)
        .border(1.0, BORDER_CARD, rect_shape())
        .padding_relative(3.0, 2.0, 2.0, 2.0);
    match level % 3 {
        0 => {
            Box(m, BoxSpec::default(), move || {
                DeepNestedChain(level + 1, max_depth, Modifier::empty())
            });
        }
        1 => {
            Row(m, RowSpec::default(), move || {
                DeepNestedChain(level + 1, max_depth, Modifier::empty().rowWeight(1.0, true))
            });
        }
        _ => {
            Column(m, ColumnSpec::default(), move || {
                DeepNestedChain(
                    level + 1,
                    max_depth,
                    Modifier::empty().columnWeight(1.0, true),
                )
            });
        }
    }
}
