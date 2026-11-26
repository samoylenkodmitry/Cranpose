use std::{cell::RefCell, rc::Rc};

use cranpose::prelude::*;
use cranpose_foundation::lazy::{
    LazyListScope, LazyListState, rememberLazyListState, rememberLazyListStateWithPosition,
};
use cranpose_ui::{AnimateScroll, ScrollState};
use cranpose_ui_graphics::Color;

use super::{LazyChurnConfig, StateSlot, make_shell, run_toggleable_lazy_column, step_frames};

#[composable]
fn FillerStack(count: usize, height: f32, bg: Option<Color>) {
    for _ in 0..count {
        let mut m = Modifier::empty().fill_max_width().height(height);
        if let Some(color) = bg {
            m = m.background(color);
        }
        Box(m, BoxSpec::default(), || {});
    }
}

#[composable]
fn HorizontalChipStrip(count: usize, width: f32, height: f32, bg: Option<Color>) {
    for _ in 0..count {
        let mut m = Modifier::empty().size_points(width, height);
        if let Some(color) = bg {
            m = m.background(color);
        }
        Box(m, BoxSpec::default(), || {});
    }
}

pub(super) fn toggle_local_scroll_state() {
    let show_cell: StateSlot<MutableState<bool>> = Rc::new(RefCell::new(None));
    let cap = Rc::clone(&show_cell);
    let mut shell = make_shell(400, 300, 1.0, move || {
        let show = rememberMutableStateOf(|| true);
        *cap.borrow_mut() = Some(show);
        if show.get() {
            let vs = remember(|| ScrollState::new(20.0)).with(Clone::clone);
            let hs = remember(|| ScrollState::new(15.0)).with(Clone::clone);
            Column(
                Modifier::empty().fill_max_size().vertical_scroll(vs, false),
                ColumnSpec::default(),
                move || {
                    Row(
                        Modifier::empty()
                            .fill_max_width()
                            .horizontal_scroll(hs, false),
                        RowSpec::default(),
                        || {
                            HorizontalChipStrip(12, 60.0, 40.0, Some(Color::BLUE));
                        },
                    );
                    FillerStack(20, 30.0, Some(Color::from_rgb_u8(0x88, 0x88, 0x88)));
                },
            );
        } else {
            Box(Modifier::empty().fill_max_size(), BoxSpec::default(), || {});
        }
    });
    step_frames(&mut shell, 1, 1.0);
    for i in 1..=10 {
        if let Some(show) = *show_cell.borrow() {
            show.set(i % 2 == 0);
        }
        step_frames(&mut shell, 1, 1.0);
    }
    drop(shell);
}

pub(super) fn toggle_hoisted_scroll_state() {
    let cell: StateSlot<(MutableState<bool>, ScrollState)> = Rc::new(RefCell::new(None));
    let cap = Rc::clone(&cell);
    let mut shell = make_shell(360, 240, 1.0, move || {
        let show = rememberMutableStateOf(|| true);
        let vs = remember(|| ScrollState::new(0.0)).with(Clone::clone);
        *cap.borrow_mut() = Some((show, vs));
        if show.get() {
            Column(
                Modifier::empty().fill_max_size().vertical_scroll(vs, false),
                ColumnSpec::default(),
                || {
                    FillerStack(25, 24.0, Some(Color::BLUE));
                },
            );
        }
    });
    step_frames(&mut shell, 1, 1.0);
    for i in 1..=8 {
        let (show, vs) = (*cell.borrow()).unwrap();
        show.set(i % 2 == 0);
        step_frames(&mut shell, 1, 1.0);
        vs.scroll_to((i as f32) * 25.0);
        vs.dispatch_raw_delta(-15.0);
        step_frames(&mut shell, 1, 1.0);
    }
    drop(shell);
}

pub(super) fn swap_scroll_state() {
    let cell: StateSlot<(MutableState<bool>, ScrollState, ScrollState)> =
        Rc::new(RefCell::new(None));
    let cap = Rc::clone(&cell);
    let mut shell = make_shell(300, 200, 1.0, move || {
        let use_a = rememberMutableStateOf(|| true);
        let a = remember(|| ScrollState::new(10.0)).with(Clone::clone);
        let b = remember(|| ScrollState::new(50.0)).with(Clone::clone);
        *cap.borrow_mut() = Some((use_a, a, b));
        let active = if use_a.get() { a } else { b };
        Column(
            Modifier::empty()
                .fill_max_size()
                .vertical_scroll(active, false),
            ColumnSpec::default(),
            || {
                FillerStack(20, 25.0, None);
            },
        );
    });
    step_frames(&mut shell, 1, 1.0);
    for i in 1..=8 {
        let (use_a, a, b) = (*cell.borrow()).unwrap();
        use_a.set(i % 2 == 0);
        a.dispatch_raw_delta(-12.0);
        b.dispatch_raw_delta(12.0);
        step_frames(&mut shell, 1, 1.0);
    }
    drop(shell);
}

pub(super) fn nested_extreme_deltas() {
    let cell: StateSlot<(LazyListState, LazyListState, ScrollState)> = Rc::new(RefCell::new(None));
    let cap = Rc::clone(&cell);
    let mut shell = make_shell(360, 240, 1.0, move || {
        let lc = rememberLazyListState();
        let lr = rememberLazyListState();
        let hs = remember(|| ScrollState::new(0.0)).with(Clone::clone);
        *cap.borrow_mut() = Some((lc, lr, hs));
        LazyColumn(
            Modifier::empty().fill_max_size(),
            lc,
            LazyColumnSpec::default(),
            move |scope| {
                scope.item(move || {
                    LazyRow(
                        Modifier::empty().fill_max_width(),
                        lr,
                        LazyRowSpec::default(),
                        |s| {
                            s.items(20, |_| {
                                Box(
                                    Modifier::empty().size_points(50.0, 28.0),
                                    BoxSpec::default(),
                                    || {},
                                );
                            });
                        },
                    );
                });
                scope.item(move || {
                    Row(
                        Modifier::empty()
                            .fill_max_width()
                            .horizontal_scroll(hs, false),
                        RowSpec::default(),
                        || {
                            HorizontalChipStrip(15, 48.0, 24.0, None);
                        },
                    );
                });
                scope.items(20, |_| {
                    Box(
                        Modifier::empty().fill_max_width().height(30.0),
                        BoxSpec::default(),
                        || {},
                    );
                });
            },
        );
    });
    step_frames(&mut shell, 1, 1.0);
    let (lc, lr, hs) = (*cell.borrow()).unwrap();
    for d in [-5000.0, 5000.0, 0.0, -123.5, 99999.0, -99999.0] {
        lc.dispatch_scroll_delta(d);
        lr.dispatch_scroll_delta(-d);
        hs.dispatch_raw_delta(d);
        step_frames(&mut shell, 1, 1.0);
    }
    drop(shell);
}

pub(super) fn animated_scroll_unmount() {
    let cell: StateSlot<(MutableState<bool>, ScrollState, LazyListState)> =
        Rc::new(RefCell::new(None));
    let cap = Rc::clone(&cell);
    let mut shell = make_shell(360, 240, 1.0, move || {
        let show = rememberMutableStateOf(|| true);
        if show.get() {
            let vs = remember(|| ScrollState::new(0.0)).with(Clone::clone);
            let lc = rememberLazyListStateWithPosition(0, 0.0);
            *cap.borrow_mut() = Some((show, vs, lc));
            Row(
                Modifier::empty().fill_max_size(),
                RowSpec::default(),
                move || {
                    Column(
                        Modifier::empty()
                            .rowWeight(1.0, true)
                            .fill_max_height()
                            .vertical_scroll(vs, false),
                        ColumnSpec::default(),
                        || {
                            FillerStack(30, 24.0, None);
                        },
                    );
                    LazyColumn(
                        Modifier::empty().rowWeight(1.0, true).fill_max_height(),
                        lc,
                        LazyColumnSpec::default(),
                        |s| {
                            s.items(30, |_| {
                                Box(
                                    Modifier::empty().fill_max_width().height(24.0),
                                    BoxSpec::default(),
                                    || {},
                                );
                            });
                        },
                    );
                },
            );
        } else {
            Box(Modifier::empty().fill_max_size(), BoxSpec::default(), || {});
        }
    });
    step_frames(&mut shell, 1, 1.0);
    let (show, vs, lc) = (*cell.borrow()).unwrap();
    vs.animate_scroll_to(300.0);
    lc.animate_scroll_to_item(20, 0.0);
    step_frames(&mut shell, 2, 1.0);
    show.set(false);
    step_frames(&mut shell, 2, 1.0);
    drop(shell);
}

pub(super) fn conditional_keyed_scroll_detach() {
    let cell: StateSlot<MutableState<bool>> = Rc::new(RefCell::new(None));
    let cap = Rc::clone(&cell);
    let mut shell = make_shell(320, 240, 1.0, move || {
        let enabled = rememberMutableStateOf(|| true);
        *cap.borrow_mut() = Some(enabled);
        let mut scroll_mod = Modifier::empty().fill_max_size();
        if enabled.get() {
            let s = cranpose_core::key(1601u64, || {
                remember(|| ScrollState::new(10.0)).with(Clone::clone)
            });
            scroll_mod = scroll_mod.vertical_scroll(s, false);
        }
        Column(scroll_mod, ColumnSpec::default(), || {
            FillerStack(20, 24.0, Some(Color::BLUE))
        });
    });
    step_frames(&mut shell, 1, 1.0);
    for i in 1..=6 {
        cell.borrow().unwrap().set(i % 2 == 0);
        step_frames(&mut shell, 1, 1.0);
    }
    drop(shell);
}

pub(super) fn reuse_pool_eviction() {
    run_toggleable_lazy_column(
        LazyChurnConfig {
            width: 320,
            height: 160,
            count: 100,
            spacing: 0.0,
            scroll_targets: &[15, 35, 55, 75, 95, 40, 0, 88],
            scroll_offset: 5.0,
            remount_after_hide: true,
        },
        |i| {
            let hs = remember(|| ScrollState::new((i % 20) as f32)).with(Clone::clone);
            Row(
                Modifier::empty()
                    .fill_max_width()
                    .horizontal_scroll(hs, false),
                RowSpec::default(),
                || {
                    HorizontalChipStrip(8, 60.0, 28.0, Some(Color::BLUE));
                },
            );
        },
    );
}

#[composable]
fn SplitOwnerAndScrollNode(
    keep_owner: bool,
    keep_node: bool,
    owner_key: u64,
    node_key: u64,
    shared: StateSlot<ScrollState>,
) {
    let sh1 = Rc::clone(&shared);
    let sh2 = Rc::clone(&shared);
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            if keep_owner {
                let sh1_inner = Rc::clone(&sh1);
                cranpose_core::key(owner_key, move || {
                    let owned = remember(|| ScrollState::new(12.0)).with(Clone::clone);
                    *sh1_inner.borrow_mut() = Some(owned);
                });
            }
            if keep_node {
                let sh2_inner = Rc::clone(&sh2);
                cranpose_core::key(node_key, move || {
                    if let Some(s) = *sh2_inner.borrow() {
                        Column(
                            Modifier::empty()
                                .fill_max_width()
                                .height(100.0)
                                .vertical_scroll(s, false),
                            ColumnSpec::default(),
                            || FillerStack(10, 24.0, None),
                        );
                    }
                });
            }
        },
    );
}

pub(super) fn same_frame_owner_and_node_removal() {
    let cell: StateSlot<MutableState<bool>> = Rc::new(RefCell::new(None));
    let cap = Rc::clone(&cell);
    let shared: StateSlot<ScrollState> = Rc::new(RefCell::new(None));
    let mut shell = make_shell(320, 240, 1.0, move || {
        let mounted = rememberMutableStateOf(|| true);
        *cap.borrow_mut() = Some(mounted);
        let live = mounted.get();
        SplitOwnerAndScrollNode(live, live, 1801, 1802, Rc::clone(&shared));
    });
    step_frames(&mut shell, 1, 1.0);
    cell.borrow().unwrap().set(false);
    step_frames(&mut shell, 1, 1.0);
    drop(shell);
}

pub(super) fn owner_disposed_before_node_detaches() {
    let cell: StateSlot<(MutableState<bool>, MutableState<bool>)> = Rc::new(RefCell::new(None));
    let cap = Rc::clone(&cell);
    let shared: Rc<RefCell<Option<ScrollState>>> = Rc::new(RefCell::new(None));
    let mut shell = make_shell(320, 240, 1.0, move || {
        let keep_owner = rememberMutableStateOf(|| true);
        let keep_node = rememberMutableStateOf(|| true);
        *cap.borrow_mut() = Some((keep_owner, keep_node));
        SplitOwnerAndScrollNode(
            keep_owner.get(),
            keep_node.get(),
            2001,
            2002,
            Rc::clone(&shared),
        );
    });
    step_frames(&mut shell, 1, 1.0);
    let (keep_owner, keep_node) = cell.borrow().unwrap();
    keep_owner.set(false);
    step_frames(&mut shell, 1, 1.0);
    keep_node.set(false);
    step_frames(&mut shell, 1, 1.0);
    drop(shell);
}
