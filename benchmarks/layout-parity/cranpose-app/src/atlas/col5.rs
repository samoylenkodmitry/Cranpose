use std::rc::Rc;

use cranpose::{LazyItems, prelude::*};
use cranpose_foundation::lazy::{LazyListScope, rememberLazyListState};
use cranpose_ui::{
    PaddingValues, ScrollState,
    widgets::{
        BoxWithConstraints, BoxWithConstraintsScope, FlowRow, FlowSpec, GridCells, HorizontalPager,
        LazyVerticalGrid, LazyVerticalGridSpec, LazyVerticalStaggeredGrid,
        LazyVerticalStaggeredGridSpec, PageSize, PagerSpec, rememberLazyGridState,
        rememberLazyStaggeredGridState, rememberPagerState,
    },
};
use cranpose_ui_graphics::{Color, GraphicsLayer, Size};
use cranpose_ui_layout::{Alignment, IntrinsicSize, LinearArrangement, VerticalAlignment};

use super::common::{
    ACCENTS, AMBER, AMBER_SOFT, BG_SUBTLE, BG_TRACK, BLUE, BLUE_SOFT, BORDER_STRONG, CYAN,
    CYAN_SOFT, ChipSpec, EMERALD, EMERALD_SOFT, FullWidthChip, INDIGO, INDIGO_SOFT, INK,
    LabeledChip, RED, ROSE, ROSE_SOFT, RowChipSpec, SLATE, TEAL, TEAL_SOFT, VIOLET, VIOLET_SOFT,
    ZoneCard, atlas_text_style, rect_shape,
};

#[composable]
pub(super) fn AtlasCol5(modifier: Modifier) {
    Column(
        modifier.fill_max_height(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
        || {
            Zone17LazyRowInLazyColumn(Modifier::empty().fill_max_width().columnWeight(1.08, true));
            Zone18WrappedContainers(Modifier::empty().fill_max_width().columnWeight(1.06, true));
            Zone19LazyColInLazyRow(Modifier::empty().fill_max_width().columnWeight(0.96, true));
            Zone20ModifierPerms(Modifier::empty().fill_max_width().columnWeight(0.90, true));
        },
    );
}

fn add_z17_fixed_and_wrap_items(scope: &mut impl LazyListScope) {
    scope.item_keyed(Some(1700), None, || {
        let st = rememberLazyListState();
        LazyRow(
            Modifier::empty()
                .test_tag("z17_lr_fixed_h")
                .fill_max_width()
                .height(30.0)
                .background(BLUE_SOFT)
                .border(1.0, BLUE, rect_shape()),
            st,
            LazyRowSpec::new()
                .content_padding(PaddingValues::symmetric(5.0, 3.0))
                .horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
            |s| {
                s.items(LazyItems::new(6).key(|i| i as u64), |i| {
                    LabeledChip(
                        format!("z17_f{i}"),
                        format!("FH{i}"),
                        ChipSpec {
                            width: 44.0,
                            height: 22.0,
                            bg: ACCENTS[i % ACCENTS.len()],
                            radius: 3.0,
                            font_size: 7.5,
                        },
                    );
                });
            },
        );
    });
    scope.item_keyed(Some(1701), None, || {
        let st = rememberLazyListState();
        let hs: Rc<[f32]> = Rc::from([16.0, 28.0, 20.0, 24.0, 18.0, 26.0]);
        LazyRow(
            Modifier::empty()
                .test_tag("z17_lr_wrap_h")
                .fill_max_width()
                .background(AMBER_SOFT)
                .border(1.0, AMBER, rect_shape()),
            st,
            LazyRowSpec::new()
                .content_padding(PaddingValues::symmetric(5.0, 3.0))
                .horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
            move |s| {
                let hs = Rc::clone(&hs);
                s.items(LazyItems::new(hs.len()).key(|i| i as u64), move |i| {
                    LabeledChip(
                        format!("z17_w{i}"),
                        format!("WH{i}"),
                        ChipSpec {
                            width: 42.0,
                            height: hs[i],
                            bg: ACCENTS[(i + 2) % ACCENTS.len()],
                            radius: 3.0,
                            font_size: 7.5,
                        },
                    );
                });
            },
        );
    });
}

fn add_z17_perm_items(scope: &mut impl LazyListScope) {
    scope.item_keyed(Some(1702), None, || {
        let st = rememberLazyListState();
        LazyRow(
            Modifier::empty()
                .test_tag("z17_lr_perm_a")
                .padding(4.0)
                .background(EMERALD_SOFT)
                .border(1.0, EMERALD, rect_shape())
                .fill_max_width(),
            st,
            LazyRowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
            |s| {
                s.items(LazyItems::new(6).key(|i| i as u64), |i| {
                    LabeledChip(
                        format!("z17_pa{i}"),
                        format!("PA{i}"),
                        ChipSpec {
                            width: 38.0,
                            height: 18.0,
                            bg: EMERALD,
                            radius: 3.0,
                            font_size: 7.5,
                        },
                    );
                });
            },
        );
    });
    scope.item_keyed(Some(1703), None, || {
        let st = rememberLazyListState();
        let hs: Rc<[f32]> = Rc::from([16.0, 40.0, 22.0, 18.0, 26.0]);
        LazyRow(
            Modifier::empty()
                .test_tag("z17_lr_perm_b")
                .fill_max_width()
                .background(VIOLET_SOFT)
                .padding(3.0)
                .border(1.0, VIOLET, rect_shape())
                .size_in(Size::new(0.0, 24.0), Size::new(f32::INFINITY, 32.0)),
            st,
            LazyRowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
            move |s| {
                let hs = Rc::clone(&hs);
                s.items(LazyItems::new(hs.len()).key(|i| i as u64), move |i| {
                    LabeledChip(
                        format!("z17_pb{i}"),
                        format!("PB{i}"),
                        ChipSpec {
                            width: 40.0,
                            height: hs[i],
                            bg: VIOLET,
                            radius: 3.0,
                            font_size: 7.5,
                        },
                    );
                });
            },
        );
    });
    scope.item_keyed(Some(1704), None, || {
        let st = rememberLazyListState();
        LazyRow(
            Modifier::empty()
                .test_tag("z17_lr_rev_wrap")
                .fill_max_width()
                .background(ROSE_SOFT)
                .border(1.0, ROSE, rect_shape()),
            st,
            LazyRowSpec::new()
                .content_padding(PaddingValues::symmetric(4.0, 3.0))
                .reverse_layout(true)
                .horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
            |s| {
                s.items(LazyItems::new(6).key(|i| i as u64), |i| {
                    LabeledChip(
                        format!("z17_rv{i}"),
                        format!("RV{i}"),
                        ChipSpec {
                            width: 40.0,
                            height: 20.0,
                            bg: ROSE,
                            radius: 3.0,
                            font_size: 7.5,
                        },
                    );
                });
            },
        );
    });
}

#[composable]
fn Zone17LazyRowInLazyColumn(modifier: Modifier) {
    let outer_state = rememberLazyListState();
    ZoneCard(
        "z17_card",
        "17. LazyRow in LazyColumn (With/Without height · Perms)",
        INDIGO,
        modifier,
        move || {
            LazyColumn(
                Modifier::empty()
                    .test_tag("z17_outer_lcol")
                    .fill_max_size()
                    .background(BG_SUBTLE)
                    .border(1.0, BORDER_STRONG, rect_shape()),
                outer_state,
                LazyColumnSpec::new()
                    .content_padding_all(4.0)
                    .vertical_arrangement(LinearArrangement::spaced_by(4.0)),
                |scope| {
                    add_z17_fixed_and_wrap_items(scope);
                    add_z17_perm_items(scope);
                },
            );
        },
    );
}

#[composable]
fn Zone18WrappedContainers(modifier: Modifier) {
    ZoneCard(
        "z18_card",
        "18. Wrapped: Scroll/Flow/BWC/Intr/Pager in Lazy & VScroll",
        BLUE,
        modifier,
        || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(5.0)),
                || {
                    Zone18MixedLazyColumn();
                    Zone18LazyRowInVScroll();
                },
            );
        },
    );
}

fn add_z18_scroll_and_flow_items(scope: &mut impl LazyListScope) {
    scope.item_keyed(Some(1800), None, || {
        let hs = remember(|| ScrollState::new(18.0)).with(Clone::clone);
        Row(
            Modifier::empty()
                .test_tag("z18_hscroll_in_lc")
                .fill_max_width()
                .background(CYAN_SOFT)
                .border(1.0, CYAN, rect_shape())
                .horizontal_scroll(hs, false)
                .padding(3.0),
            RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
            || {
                for i in 0..6 {
                    LabeledChip(
                        format!("z18_hs{i}"),
                        format!("HS{i}"),
                        ChipSpec {
                            width: 46.0,
                            height: 18.0,
                            bg: ACCENTS[i % ACCENTS.len()],
                            radius: 3.0,
                            font_size: 7.5,
                        },
                    );
                }
            },
        );
    });
    scope.item_keyed(Some(1801), None, || {
        FlowRow(
            Modifier::empty()
                .test_tag("z18_flow_in_lc")
                .fill_max_width()
                .background(EMERALD_SOFT)
                .border(1.0, EMERALD, rect_shape())
                .padding(3.0),
            FlowSpec::new()
                .main_axis_spacing(4.0)
                .cross_axis_spacing(3.0),
            || {
                for (i, w) in [54.0, 62.0, 46.0, 70.0, 50.0, 58.0].into_iter().enumerate() {
                    LabeledChip(
                        format!("z18_fl{i}"),
                        format!("F{i}"),
                        ChipSpec {
                            width: w,
                            height: 14.0,
                            bg: EMERALD,
                            radius: 2.0,
                            font_size: 7.0,
                        },
                    );
                }
            },
        );
    });
}

fn add_z18_bwc_intr_pager_items(scope: &mut impl LazyListScope) {
    scope.item_keyed(Some(1802), None, || {
        BoxWithConstraints(
            Modifier::empty()
                .test_tag("z18_bwc_in_lc")
                .fill_max_width()
                .background(INDIGO_SOFT)
                .border(1.0, INDIGO, rect_shape())
                .padding(3.0),
            |bwc| {
                let mw = bwc.max_width().0;
                Box(
                    Modifier::empty()
                        .test_tag("z18_bwc_bar")
                        .width(mw * 0.55)
                        .height(14.0)
                        .background(INDIGO),
                    BoxSpec::new().content_alignment(Alignment::CENTER_START),
                    || {
                        Text(
                            " BWC-in-Lazy 55%",
                            Modifier::empty(),
                            atlas_text_style(7.5, Color::WHITE, true),
                        );
                    },
                );
            },
        );
    });
    scope.item_keyed(Some(1803), None, || {
        Row(
            Modifier::empty()
                .test_tag("z18_intr_in_lc")
                .fill_max_width()
                .height_intrinsic(IntrinsicSize::Min)
                .background(AMBER_SOFT)
                .border(1.0, AMBER, rect_shape())
                .padding(3.0),
            RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(5.0)),
            || {
                Text(
                    "Intrinsics inside lazy item wrap cleanly.",
                    Modifier::empty().rowWeight(1.0, true),
                    atlas_text_style(8.0, INK, false),
                );
                Box(
                    Modifier::empty()
                        .test_tag("z18_intr_div")
                        .width(3.0)
                        .fill_max_height()
                        .background(RED),
                    BoxSpec::default(),
                    || {},
                );
                Text(
                    "OK",
                    Modifier::empty().alignInRow(VerticalAlignment::CenterVertically),
                    atlas_text_style(8.0, AMBER, true),
                );
            },
        );
    });
    scope.item_keyed(Some(1804), None, || {
        let ps = rememberPagerState(0);
        HorizontalPager(
            Modifier::empty()
                .test_tag("z18_pager_in_lc")
                .fill_max_width()
                .height(26.0)
                .background(VIOLET_SOFT)
                .border(1.0, VIOLET, rect_shape()),
            ps,
            PagerSpec {
                page_size: PageSize::Fixed(86.0),
                page_spacing: 4.0,
                content_padding: PaddingValues::symmetric(4.0, 2.0),
                ..PagerSpec::default()
            },
            4,
            |p| {
                Box(
                    Modifier::empty()
                        .test_tag(format!("z18_pg{p}"))
                        .fill_max_size()
                        .background(ACCENTS[(p + 4) % ACCENTS.len()])
                        .rounded_corners(3.0),
                    BoxSpec::new().content_alignment(Alignment::CENTER),
                    move || {
                        Text(
                            format!("LPg{p}"),
                            Modifier::empty(),
                            atlas_text_style(7.5, Color::WHITE, true),
                        );
                    },
                );
            },
        );
    });
}

#[composable]
fn Zone18MixedLazyColumn() {
    let outer_lazy = rememberLazyListState();
    LazyColumn(
        Modifier::empty()
            .test_tag("z18_mix_lcol")
            .fill_max_width()
            .columnWeight(1.38, true)
            .background(BG_SUBTLE)
            .border(1.0, BORDER_STRONG, rect_shape()),
        outer_lazy,
        LazyColumnSpec::new()
            .content_padding_all(4.0)
            .vertical_arrangement(LinearArrangement::spaced_by(4.0)),
        |scope| {
            add_z18_scroll_and_flow_items(scope);
            add_z18_bwc_intr_pager_items(scope);
        },
    );
}

#[composable]
fn Zone18LazyRowInVScroll() {
    let outer_vscroll = remember(|| ScrollState::new(0.0)).with(Clone::clone);
    Column(
        Modifier::empty()
            .test_tag("z18_vscroll_outer")
            .fill_max_width()
            .columnWeight(0.62, true)
            .background(BG_SUBTLE)
            .border(1.0, BORDER_STRONG, rect_shape())
            .vertical_scroll(outer_vscroll, false)
            .padding(3.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(4.0)),
        || {
            let lr_state = rememberLazyListState();
            let hs: Rc<[f32]> = Rc::from([16.0, 24.0, 18.0, 22.0, 16.0, 20.0]);
            LazyRow(
                Modifier::empty()
                    .test_tag("z18_lr_in_vscroll")
                    .fill_max_width()
                    .background(CYAN_SOFT)
                    .border(1.0, CYAN, rect_shape()),
                lr_state,
                LazyRowSpec::new()
                    .content_padding_all(3.0)
                    .horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
                move |s| {
                    let hs = Rc::clone(&hs);
                    s.items(LazyItems::new(hs.len()).key(|i| i as u64), move |i| {
                        LabeledChip(
                            format!("z18_lvs{i}"),
                            format!("VS{i}"),
                            ChipSpec {
                                width: 44.0,
                                height: hs[i],
                                bg: CYAN,
                                radius: 3.0,
                                font_size: 7.5,
                            },
                        );
                    });
                },
            );
            Row(
                Modifier::empty(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
                || {
                    for i in 0..3 {
                        LabeledChip(
                            format!("z18_vs_tail{i}"),
                            format!("Tail{i}"),
                            ChipSpec {
                                width: 54.0,
                                height: 16.0,
                                bg: SLATE,
                                radius: 2.0,
                                font_size: 7.5,
                            },
                        );
                    }
                },
            );
        },
    );
}

#[composable]
fn Zone19LazyColInLazyRow(modifier: Modifier) {
    ZoneCard(
        "z19_card",
        "19. LazyCol/VScroll in LazyRow · AdaptiveGrid in BWC",
        EMERALD,
        modifier,
        || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(5.0)),
                || {
                    Zone19OuterLazyRow();
                    Zone19AdaptiveAndStaggered();
                },
            );
        },
    );
}

#[composable]
fn Zone19FixedLazyCol() {
    let st = rememberLazyListState();
    LazyColumn(
        Modifier::empty()
            .test_tag("z19_lc_fixed_w")
            .width(102.0)
            .fill_max_height()
            .background(BLUE_SOFT)
            .border(1.0, BLUE, rect_shape()),
        st,
        LazyColumnSpec::new()
            .content_padding_all(3.0)
            .vertical_arrangement(LinearArrangement::spaced_by(3.0)),
        |s| {
            s.items(LazyItems::new(5).key(|i| i as u64), |i| {
                FullWidthChip(
                    format!("z19_lcf{i}"),
                    format!("LC-F{i}"),
                    RowChipSpec {
                        height: 18.0,
                        bg: BLUE,
                        radius: 2.0,
                        font_size: 7.5,
                        alignment: Alignment::CENTER,
                        horizontal_padding: 0.0,
                    },
                );
            });
        },
    );
}

#[composable]
fn Zone19WrapLazyCol() {
    let st = rememberLazyListState();
    let ws: Rc<[f32]> = Rc::from([52.0, 86.0, 68.0, 60.0]);
    LazyColumn(
        Modifier::empty()
            .test_tag("z19_lc_wrap_w")
            .width_in(56.0, 104.0)
            .fill_max_height()
            .background(EMERALD_SOFT)
            .border(1.0, EMERALD, rect_shape()),
        st,
        LazyColumnSpec::new()
            .content_padding_all(3.0)
            .vertical_arrangement(LinearArrangement::spaced_by(3.0)),
        move |s| {
            let ws = Rc::clone(&ws);
            s.items(LazyItems::new(ws.len()).key(|i| i as u64), move |i| {
                let w = ws[i];
                LabeledChip(
                    format!("z19_lcw{i}"),
                    format!("W{}", w as i32),
                    ChipSpec {
                        width: w,
                        height: 18.0,
                        bg: EMERALD,
                        radius: 2.0,
                        font_size: 7.5,
                    },
                );
            });
        },
    );
}

#[composable]
fn Zone19VScrollItem() {
    let vs = remember(|| ScrollState::new(12.0)).with(Clone::clone);
    Column(
        Modifier::empty()
            .test_tag("z19_vs_in_lr")
            .width(88.0)
            .fill_max_height()
            .background(AMBER_SOFT)
            .border(1.0, AMBER, rect_shape())
            .vertical_scroll(vs, false)
            .padding(3.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(3.0)),
        || {
            for i in 0..5 {
                FullWidthChip(
                    format!("z19_vslr{i}"),
                    format!("VS-R{i}"),
                    RowChipSpec {
                        height: 18.0,
                        bg: AMBER,
                        radius: 2.0,
                        font_size: 7.5,
                        alignment: Alignment::CENTER,
                        horizontal_padding: 0.0,
                    },
                );
            }
        },
    );
}

#[composable]
fn Zone19OuterLazyRow() {
    let outer_row = rememberLazyListState();
    LazyRow(
        Modifier::empty()
            .test_tag("z19_outer_lrow")
            .fill_max_width()
            .height(82.0)
            .background(BG_SUBTLE)
            .border(1.0, BORDER_STRONG, rect_shape()),
        outer_row,
        LazyRowSpec::new()
            .content_padding_all(4.0)
            .horizontal_arrangement(LinearArrangement::spaced_by(6.0)),
        |scope| {
            scope.item_keyed(Some(1900), None, || Zone19FixedLazyCol());
            scope.item_keyed(Some(1901), None, || Zone19WrapLazyCol());
            scope.item_keyed(Some(1902), None, || Zone19VScrollItem());
        },
    );
}

#[composable]
fn Zone19AdaptiveAndStaggered() {
    Row(
        Modifier::empty().fill_max_width().columnWeight(1.0, true),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(6.0)),
        || {
            BoxWithConstraints(
                Modifier::empty()
                    .rowWeight(1.1, true)
                    .test_tag("z19_bwc_grid")
                    .fill_max_height()
                    .background(VIOLET_SOFT)
                    .border(1.0, VIOLET, rect_shape()),
                |_bwc| {
                    let gs = rememberLazyGridState();
                    LazyVerticalGrid(
                        Modifier::empty().test_tag("z19_adapt_grid").fill_max_size(),
                        gs,
                        LazyVerticalGridSpec::new(GridCells::Adaptive(52.0))
                            .spacing(3.0)
                            .content_padding(PaddingValues::all(3.0)),
                        |s| {
                            s.items(LazyItems::new(6).key(|i| i as u64), |i| {
                                FullWidthChip(
                                    format!("z19_ag{i}"),
                                    format!("A{i}"),
                                    RowChipSpec {
                                        height: 18.0,
                                        bg: ACCENTS[i % ACCENTS.len()],
                                        radius: 2.0,
                                        font_size: 7.5,
                                        alignment: Alignment::CENTER,
                                        horizontal_padding: 0.0,
                                    },
                                );
                            });
                        },
                    );
                },
            );
            let ss = rememberLazyStaggeredGridState();
            let sh: Rc<[f32]> = Rc::from([18.0, 28.0, 22.0, 16.0, 24.0]);
            LazyVerticalStaggeredGrid(
                Modifier::empty()
                    .rowWeight(0.9, true)
                    .test_tag("z19_row_stag")
                    .fill_max_height()
                    .background(TEAL_SOFT)
                    .border(1.0, TEAL, rect_shape()),
                ss,
                LazyVerticalStaggeredGridSpec::new(GridCells::Fixed(2))
                    .spacing(3.0)
                    .content_padding(PaddingValues::all(3.0)),
                move |s| {
                    let sh = Rc::clone(&sh);
                    s.items(LazyItems::new(sh.len()).key(|i| i as u64), move |i| {
                        FullWidthChip(
                            format!("z19_rs{i}"),
                            format!("S{i}"),
                            RowChipSpec {
                                height: sh[i],
                                bg: TEAL,
                                radius: 2.0,
                                font_size: 7.5,
                                alignment: Alignment::CENTER,
                                horizontal_padding: 0.0,
                            },
                        );
                    });
                },
            );
        },
    );
}

#[composable]
fn Zone20ModifierPerms(modifier: Modifier) {
    ZoneCard(
        "z20_card",
        "20. Modifier Permutations (Pad/Size/Clip/Offset/Layer/Scroll)",
        ROSE,
        modifier,
        || {
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
                || {
                    Zone20RowA();
                    Zone20RowB();
                    Zone20ScrollPermRow();
                },
            );
        },
    );
}

#[composable]
fn Zone20RowA() {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(6.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        || {
            Box(
                Modifier::empty()
                    .test_tag("z20_p1")
                    .padding(5.0)
                    .background(BLUE_SOFT)
                    .border(1.0, BLUE, rect_shape())
                    .size_points(52.0, 22.0),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text("P->S", Modifier::empty(), atlas_text_style(7.5, BLUE, true));
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z20_p2")
                    .size_points(52.0, 22.0)
                    .padding(5.0)
                    .background(BLUE_SOFT)
                    .border(1.0, BLUE, rect_shape()),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text("S->P", Modifier::empty(), atlas_text_style(7.5, BLUE, true));
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z20_p3")
                    .background(AMBER_SOFT)
                    .padding(4.0)
                    .border(1.0, AMBER, rect_shape())
                    .padding(3.0)
                    .background(EMERALD_SOFT)
                    .size_points(44.0, 16.0),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text("BgPBP", Modifier::empty(), atlas_text_style(7.0, INK, true));
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z20_p4")
                    .required_size(Size::new(52.0, 22.0))
                    .padding(4.0)
                    .background(VIOLET_SOFT)
                    .border(1.0, VIOLET, rect_shape()),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "Req->P",
                        Modifier::empty(),
                        atlas_text_style(7.0, VIOLET, true),
                    );
                },
            );
        },
    );
}

#[composable]
fn Zone20RowB() {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(10.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        || {
            Box(
                Modifier::empty()
                    .test_tag("z20_p5")
                    .size_points(54.0, 24.0)
                    .background(BG_TRACK)
                    .border(1.0, SLATE, rect_shape())
                    .clip_to_bounds()
                    .offset(12.0, 6.0)
                    .background(ROSE),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .test_tag("z20_p6")
                    .size_points(54.0, 24.0)
                    .background(BG_TRACK)
                    .border(1.0, SLATE, rect_shape())
                    .offset(12.0, 6.0)
                    .clip_to_bounds()
                    .background(CYAN),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .test_tag("z20_p7")
                    .graphics_layer_value(GraphicsLayer {
                        rotation_z: 10.0,
                        ..GraphicsLayer::DEFAULT
                    })
                    .padding(4.0)
                    .background(INDIGO_SOFT)
                    .border(1.0, INDIGO, rect_shape())
                    .size_points(48.0, 18.0),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "GL->P",
                        Modifier::empty(),
                        atlas_text_style(7.5, INDIGO, true),
                    );
                },
            );
            Box(
                Modifier::empty()
                    .test_tag("z20_p8")
                    .padding(4.0)
                    .graphics_layer_value(GraphicsLayer {
                        rotation_z: 10.0,
                        ..GraphicsLayer::DEFAULT
                    })
                    .background(TEAL_SOFT)
                    .border(1.0, TEAL, rect_shape())
                    .size_points(48.0, 18.0),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                || {
                    Text(
                        "P->GL",
                        Modifier::empty(),
                        atlas_text_style(7.5, TEAL, true),
                    );
                },
            );
        },
    );
}

#[composable]
fn ScrollPermColumn(
    tag_prefix: &'static str,
    label_prefix: &'static str,
    accent: Color,
    scroll_first: bool,
) {
    let state = remember(|| ScrollState::new(10.0)).with(Clone::clone);
    let base = Modifier::empty()
        .rowWeight(1.0, true)
        .test_tag(tag_prefix)
        .fill_max_height()
        .background(BG_SUBTLE)
        .border(1.0, accent, rect_shape());
    let ordered = if scroll_first {
        base.vertical_scroll(state, false).padding(5.0)
    } else {
        base.padding(5.0).vertical_scroll(state, false)
    };
    Column(
        ordered,
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(3.0)),
        move || {
            for i in 0..6 {
                FullWidthChip(
                    format!("{tag_prefix}_i{i}"),
                    format!("{label_prefix} {i}"),
                    RowChipSpec {
                        height: 16.0,
                        bg: accent,
                        radius: 2.0,
                        font_size: 7.5,
                        alignment: Alignment::CENTER,
                        horizontal_padding: 0.0,
                    },
                );
            }
        },
    );
}

#[composable]
fn Zone20ScrollPermRow() {
    Row(
        Modifier::empty().fill_max_width().columnWeight(1.0, true),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(6.0)),
        || {
            ScrollPermColumn("z20_p9", "Scroll->Pad", BLUE, true);
            ScrollPermColumn("z20_p10", "Pad->Scroll", ROSE, false);
        },
    );
}
