use std::{cell::RefCell, rc::Rc};

use cranpose::{LazyItems, prelude::*};
use cranpose_core::SlotId;
use cranpose_foundation::lazy::{LazyListScope, LazyListState, rememberLazyListState};
use cranpose_ui::{
    LayoutDirection, LayoutModifierMeasureResult, ProvideLayoutDirection, ScrollState,
    SubcomposeLayout, SubcomposeLayoutScope, SubcomposeMeasureScope,
    text::{ParagraphStyle, SpanStyle, TextUnit},
    widgets::{
        BoxWithConstraints, BoxWithConstraintsScope, FlowRow, FlowSpec, GridCells, HorizontalPager,
        LazyVerticalGrid, LazyVerticalGridSpec, LazyVerticalStaggeredGrid,
        LazyVerticalStaggeredGridSpec, PageSize, PagerSpec, rememberLazyGridState,
        rememberLazyStaggeredGridState, rememberPagerState,
    },
};
use cranpose_ui_graphics::{Color, GraphicsLayer, RoundedCornerShape, Size};
use cranpose_ui_layout::{Constraints, IntrinsicSize, LinearArrangement, Placement};

use super::{LazyChurnConfig, make_shell, run_toggleable_lazy_column, step_frames};
use crate::atlas::{AtlasConfig, ParityAtlasScreen};

fn default_atlas_config() -> AtlasConfig {
    AtlasConfig {
        width_dp: 1760,
        height_dp: 980,
        density_label: "1.0".to_owned(),
    }
}

pub(super) fn drop_full_atlas() {
    let cfg = default_atlas_config();
    let mut shell = make_shell(1760, 980, 1.0, move || ParityAtlasScreen(cfg.clone()));
    step_frames(&mut shell, 2, 1.0);
    drop(shell);
}

#[composable]
fn NestedLazyRowItem(fixed_height: Option<f32>, color: Color) {
    let lr = rememberLazyListState();
    let m = match fixed_height {
        Some(h) => Modifier::empty().fill_max_width().height(h),
        None => Modifier::empty().fill_max_width(),
    };
    let spec = if fixed_height.is_some() {
        LazyRowSpec::default()
    } else {
        LazyRowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(4.0))
    };
    LazyRow(m, lr, spec, move |s| {
        s.items(8, move |i| {
            let h = if fixed_height.is_some() {
                24.0
            } else {
                18.0 + (i as f32) * 2.0
            };
            Box(
                Modifier::empty().size_points(50.0, h).background(color),
                BoxSpec::default(),
                || {},
            );
        });
    });
}

#[composable]
fn NestedBwcIntrinsicItem(idx: usize) {
    BoxWithConstraints(Modifier::empty().fill_max_width(), move |bwc| {
        let mw = bwc.max_width().0;
        Row(
            Modifier::empty()
                .fill_max_width()
                .height_intrinsic(IntrinsicSize::Min),
            RowSpec::default(),
            move || {
                Text(
                    format!("Item {idx} w={}", mw as i32),
                    Modifier::empty().rowWeight(1.0, true),
                    TextStyle {
                        span_style: SpanStyle {
                            font_size: TextUnit::Sp(9.0),
                            ..Default::default()
                        },
                        paragraph_style: ParagraphStyle {
                            line_height: TextUnit::Em(1.4),
                            ..Default::default()
                        },
                    },
                );
                Box(
                    Modifier::empty()
                        .width(2.0)
                        .fill_max_height()
                        .background(Color::RED),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    });
}

#[composable]
fn NestedLazyItem(idx: usize) {
    match idx % 5 {
        0 => {
            let hs = remember(|| ScrollState::new(12.0)).with(Clone::clone);
            Row(
                Modifier::empty()
                    .fill_max_width()
                    .horizontal_scroll(hs, false),
                RowSpec::default(),
                || {
                    for _ in 0..8 {
                        Box(
                            Modifier::empty()
                                .size_points(60.0, 24.0)
                                .background(Color::BLUE),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                },
            );
        }
        1 => NestedLazyRowItem(None, Color::GREEN),
        2 => NestedLazyRowItem(Some(32.0), Color::RED),
        3 => {
            FlowRow(
                Modifier::empty().fill_max_width(),
                FlowSpec::new()
                    .main_axis_spacing(4.0)
                    .cross_axis_spacing(3.0),
                || {
                    for _ in 0..6 {
                        Box(
                            Modifier::empty()
                                .size_points(70.0, 18.0)
                                .background(Color::from_rgb_u8(0x88, 0x88, 0x88)),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                },
            );
        }
        _ => NestedBwcIntrinsicItem(idx),
    }
}

pub(super) fn lazy_col_nested_scrolls() {
    let outer_cell: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
    let cap = Rc::clone(&outer_cell);
    let mut shell = make_shell(360, 240, 1.0, move || {
        let outer = rememberLazyListState();
        *cap.borrow_mut() = Some(outer);
        LazyColumn(
            Modifier::empty().fill_max_size(),
            outer,
            LazyColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(4.0)),
            |scope| {
                scope.items(LazyItems::new(40).key(|i| i as u64), NestedLazyItem);
            },
        );
    });
    step_frames(&mut shell, 1, 1.0);
    let outer = outer_cell.borrow().unwrap();
    for (idx, off) in [(10, 5.0), (25, 12.0), (39, 0.0), (0, 0.0), (18, 8.0)] {
        outer.scroll_to_item(idx, off);
        step_frames(&mut shell, 1, 1.0);
    }
    for delta in [-400.0, -400.0, 500.0, -800.0, 1200.0] {
        outer.dispatch_scroll_delta(delta);
        step_frames(&mut shell, 1, 1.0);
    }
    drop(shell);
}

pub(super) fn lazy_row_nested_vscrolls() {
    let outer_cell: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
    let cap = Rc::clone(&outer_cell);
    let mut shell = make_shell(360, 180, 1.0, move || {
        let outer = rememberLazyListState();
        *cap.borrow_mut() = Some(outer);
        LazyRow(
            Modifier::empty().fill_max_size(),
            outer,
            LazyRowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(6.0)),
            |scope| {
                scope.items(LazyItems::new(25).key(|i| i as u64), |idx| {
                    if idx % 2 == 0 {
                        let vs = remember(|| ScrollState::new(10.0)).with(Clone::clone);
                        Column(
                            Modifier::empty()
                                .width(90.0)
                                .fill_max_height()
                                .vertical_scroll(vs, false),
                            ColumnSpec::default(),
                            || {
                                for _ in 0..10 {
                                    Box(
                                        Modifier::empty()
                                            .fill_max_width()
                                            .height(26.0)
                                            .background(Color::BLUE),
                                        BoxSpec::default(),
                                        || {},
                                    );
                                }
                            },
                        );
                    } else {
                        let lc = rememberLazyListState();
                        LazyColumn(
                            Modifier::empty().width_in(60.0, 110.0).fill_max_height(),
                            lc,
                            LazyColumnSpec::default(),
                            |s| {
                                s.items(8, |j| {
                                    Box(
                                        Modifier::empty()
                                            .size_points(55.0 + (j as f32) * 5.0, 22.0)
                                            .background(Color::GREEN),
                                        BoxSpec::default(),
                                        || {},
                                    );
                                });
                            },
                        );
                    }
                });
            },
        );
    });
    step_frames(&mut shell, 1, 1.0);
    let outer = outer_cell.borrow().unwrap();
    for (idx, off) in [(8, 4.0), (20, 0.0), (0, 0.0), (14, 15.0)] {
        outer.scroll_to_item(idx, off);
        step_frames(&mut shell, 1, 1.0);
    }
    drop(shell);
}

pub(super) fn viewport_resize_extremes() {
    let cfg = default_atlas_config();
    let mut shell = make_shell(1760, 980, 1.0, move || ParityAtlasScreen(cfg.clone()));
    for (w, h) in [
        (1760.0, 980.0),
        (640.0, 480.0),
        (160.0, 160.0),
        (1.0, 1.0),
        (0.0, 0.0),
        (1760.0, 980.0),
    ] {
        shell.set_viewport(w, h);
        shell.set_buffer_size((w as u32).max(1), (h as u32).max(1));
        step_frames(&mut shell, 1, 1.0);
    }
    drop(shell);
}

pub(super) fn runtime_density_switches() {
    let cfg = default_atlas_config();
    let mut shell = make_shell(1760, 980, 1.0, move || ParityAtlasScreen(cfg.clone()));
    for d in [1.0, 1.75, 2.625, 0.75, 3.0, 1.0] {
        shell.set_density(d);
        shell.set_buffer_size((1760.0 * d) as u32, (980.0 * d) as u32);
        step_frames(&mut shell, 1, d);
    }
    drop(shell);
}

pub(super) fn dynamic_rtl_ltr_flip() {
    let dir_cell: Rc<RefCell<Option<MutableState<LayoutDirection>>>> = Rc::new(RefCell::new(None));
    let cap = Rc::clone(&dir_cell);
    let cfg = default_atlas_config();
    let mut shell = make_shell(1760, 980, 1.0, move || {
        let dir = rememberMutableStateOf(|| LayoutDirection::Ltr);
        *cap.borrow_mut() = Some(dir);
        let cfg = cfg.clone();
        ProvideLayoutDirection(dir.get(), move || ParityAtlasScreen(cfg.clone()));
    });
    step_frames(&mut shell, 1, 1.0);
    for i in 1..=6 {
        let dir = dir_cell.borrow().unwrap();
        dir.set(if i % 2 == 0 {
            LayoutDirection::Ltr
        } else {
            LayoutDirection::Rtl
        });
        step_frames(&mut shell, 1, 1.0);
    }
    drop(shell);
}

pub(super) fn toggle_lazy_containers() {
    let phase_cell: Rc<RefCell<Option<MutableState<usize>>>> = Rc::new(RefCell::new(None));
    let cap = Rc::clone(&phase_cell);
    let mut shell = make_shell(400, 400, 1.0, move || {
        let phase = rememberMutableStateOf(|| 0usize);
        *cap.borrow_mut() = Some(phase);
        let p = phase.get();
        if p % 3 == 2 {
            Box(Modifier::empty().fill_max_size(), BoxSpec::default(), || {});
            return;
        }
        let n = if p % 3 == 0 { 0 } else { 30 };
        Column(
            Modifier::empty().fill_max_size(),
            ColumnSpec::default(),
            move || {
                let gs = rememberLazyGridState();
                LazyVerticalGrid(
                    Modifier::empty().fill_max_width().columnWeight(1.0, true),
                    gs,
                    LazyVerticalGridSpec::new(GridCells::Adaptive(60.0)).spacing(4.0),
                    move |s| {
                        s.items(n, |_| {
                            Box(
                                Modifier::empty().fill_max_width().height(20.0),
                                BoxSpec::default(),
                                || {},
                            );
                        });
                    },
                );
                let ss = rememberLazyStaggeredGridState();
                LazyVerticalStaggeredGrid(
                    Modifier::empty().fill_max_width().columnWeight(1.0, true),
                    ss,
                    LazyVerticalStaggeredGridSpec::new(GridCells::Fixed(3)).spacing(4.0),
                    move |s| {
                        s.items(n, |i| {
                            Box(
                                Modifier::empty()
                                    .fill_max_width()
                                    .height(16.0 + ((i % 4) as f32) * 8.0),
                                BoxSpec::default(),
                                || {},
                            );
                        });
                    },
                );
                let ps = rememberPagerState(0);
                HorizontalPager(
                    Modifier::empty().fill_max_width().height(40.0),
                    ps,
                    PagerSpec {
                        page_size: PageSize::Fixed(90.0),
                        ..PagerSpec::default()
                    },
                    n.min(5),
                    |_| {
                        Box(Modifier::empty().fill_max_size(), BoxSpec::default(), || {});
                    },
                );
            },
        );
    });
    for p in 0..9 {
        step_frames(&mut shell, 1, 1.0);
        if let Some(st) = *phase_cell.borrow() {
            st.set(p);
        }
    }
    drop(shell);
}

pub(super) fn hot_modifier_permutation() {
    let step_cell: Rc<RefCell<Option<MutableState<usize>>>> = Rc::new(RefCell::new(None));
    let cap = Rc::clone(&step_cell);
    let mut shell = make_shell(300, 200, 1.0, move || {
        let step = rememberMutableStateOf(|| 0usize);
        *cap.borrow_mut() = Some(step);
        let s = remember(|| ScrollState::new(5.0)).with(Clone::clone);
        let m = match step.get() % 5 {
            0 => Modifier::empty()
                .padding(4.0)
                .vertical_scroll(s, false)
                .background(Color::BLUE)
                .size_points(120.0, 80.0),
            1 => Modifier::empty()
                .size_points(120.0, 80.0)
                .graphics_layer_value(GraphicsLayer {
                    rotation_z: 8.0,
                    ..GraphicsLayer::DEFAULT
                })
                .offset(4.0, 2.0)
                .clip_to_bounds(),
            2 => Modifier::empty()
                .z_index(2.0)
                .border(1.0, Color::RED, RoundedCornerShape::uniform(0.0))
                .horizontal_scroll(s, false)
                .padding(6.0),
            3 => Modifier::empty()
                .layout(|m, c| {
                    let p = m.measure(c);
                    LayoutModifierMeasureResult::new(
                        Size::new(p.width() + 4.0, p.height() + 4.0),
                        2.0,
                        2.0,
                    )
                })
                .vertical_scroll(s, false),
            _ => Modifier::empty().padding(2.0).background(Color::GREEN),
        };
        Box(m, BoxSpec::default(), || {
            Box(
                Modifier::empty().size_points(200.0, 200.0),
                BoxSpec::default(),
                || {},
            );
        });
    });
    for i in 0..15 {
        step_frames(&mut shell, 1, 1.0);
        if let Some(st) = *step_cell.borrow() {
            st.set(i);
        }
    }
    drop(shell);
}

pub(super) fn pointer_drag_and_unmount_mid_fling() {
    let show_cell: Rc<RefCell<Option<MutableState<bool>>>> = Rc::new(RefCell::new(None));
    let cap = Rc::clone(&show_cell);
    let mut shell = make_shell(360, 240, 1.0, move || {
        let show = rememberMutableStateOf(|| true);
        *cap.borrow_mut() = Some(show);
        if !show.get() {
            Box(Modifier::empty().fill_max_size(), BoxSpec::default(), || {});
            return;
        }
        let lc = rememberLazyListState();
        LazyColumn(
            Modifier::empty().fill_max_size(),
            lc,
            LazyColumnSpec::default(),
            |scope| {
                scope.item(|| {
                    let hs = remember(|| ScrollState::new(0.0)).with(Clone::clone);
                    Row(
                        Modifier::empty()
                            .fill_max_width()
                            .horizontal_scroll(hs, false),
                        RowSpec::default(),
                        || {
                            for _ in 0..15 {
                                Box(
                                    Modifier::empty()
                                        .size_points(60.0, 40.0)
                                        .background(Color::BLUE),
                                    BoxSpec::default(),
                                    || {},
                                );
                            }
                        },
                    );
                });
                scope.items(25, |_| {
                    let lr = rememberLazyListState();
                    LazyRow(
                        Modifier::empty().fill_max_width(),
                        lr,
                        LazyRowSpec::default(),
                        |s| {
                            s.items(12, |_| {
                                Box(
                                    Modifier::empty()
                                        .size_points(50.0, 32.0)
                                        .background(Color::GREEN),
                                    BoxSpec::default(),
                                    || {},
                                );
                            });
                        },
                    );
                });
            },
        );
    });
    step_frames(&mut shell, 1, 1.0);
    shell.set_cursor(180.0, 20.0);
    shell.pointer_pressed();
    for step in 1..=5 {
        shell.set_cursor(180.0 - (step as f32) * 28.0, 20.0);
        step_frames(&mut shell, 1, 1.0);
    }
    shell.pointer_released();
    step_frames(&mut shell, 1, 1.0);
    shell.set_cursor(180.0, 180.0);
    shell.pointer_pressed();
    for step in 1..=4 {
        shell.set_cursor(180.0, 180.0 - (step as f32) * 35.0);
        step_frames(&mut shell, 1, 1.0);
    }
    shell.pointer_released();
    shell.pointer_scrolled(-40.0, -80.0);
    step_frames(&mut shell, 1, 1.0);
    show_cell.borrow().unwrap().set(false);
    step_frames(&mut shell, 2, 1.0);
    drop(shell);
}

pub(super) fn subcompose_slot_churn() {
    let tick_cell: Rc<RefCell<Option<MutableState<u64>>>> = Rc::new(RefCell::new(None));
    let cap = Rc::clone(&tick_cell);
    let mut shell = make_shell(320, 240, 1.0, move || {
        let tick = rememberMutableStateOf(|| 0u64);
        *cap.borrow_mut() = Some(tick);
        let t = tick.get();
        SubcomposeLayout(
            Modifier::empty().fill_max_size(),
            move |scope, constraints| {
                let ms = scope.subcompose(SlotId::new(t % 3), constraints, move || {
                    let vs = remember(|| ScrollState::new(8.0)).with(Clone::clone);
                    Column(
                        Modifier::empty().fill_max_size().vertical_scroll(vs, false),
                        ColumnSpec::default(),
                        || {
                            for _ in 0..12 {
                                Box(
                                    Modifier::empty()
                                        .fill_max_width()
                                        .height(32.0)
                                        .background(Color::BLUE),
                                    BoxSpec::default(),
                                    || {},
                                );
                            }
                        },
                    );
                });
                let fixed = Constraints::tight(constraints.max_width, constraints.max_height);
                let mut placements = Vec::new();
                for m in ms {
                    let p = scope.measure(m, fixed);
                    placements.push(Placement::new(p.node_id(), 0.0, 0.0, 0.0));
                }
                scope.layout(constraints.max_width, constraints.max_height, placements)
            },
        );
    });
    for t in 0..12 {
        step_frames(&mut shell, 1, 1.0);
        if let Some(st) = *tick_cell.borrow() {
            st.set(t);
        }
    }
    drop(shell);
}

pub(super) fn wrapped_lazy_row_in_lazy_col() {
    run_toggleable_lazy_column(
        LazyChurnConfig {
            width: 360,
            height: 240,
            count: 30,
            spacing: 4.0,
            scroll_targets: &[10, 25, 0, 18],
            scroll_offset: 6.0,
            remount_after_hide: false,
        },
        |i| {
            let lr = rememberLazyListState();
            let m = match i % 4 {
                0 => Modifier::empty().fill_max_width(),
                1 => Modifier::empty().fill_max_width().height(34.0),
                2 => Modifier::empty()
                    .padding(4.0)
                    .background(Color::BLUE)
                    .fill_max_width(),
                _ => Modifier::empty()
                    .fill_max_width()
                    .size_in(Size::new(0.0, 22.0), Size::new(f32::INFINITY, 36.0))
                    .padding(2.0),
            };
            LazyRow(
                m,
                lr,
                LazyRowSpec::new()
                    .reverse_layout(i % 3 == 0)
                    .horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
                |s| {
                    s.items(12, |j| {
                        Box(
                            Modifier::empty().size_points(48.0, 18.0 + ((j % 3) as f32) * 8.0),
                            BoxSpec::default(),
                            || {},
                        );
                    });
                },
            );
        },
    );
}
