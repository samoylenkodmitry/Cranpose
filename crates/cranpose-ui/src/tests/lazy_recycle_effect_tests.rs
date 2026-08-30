use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::NodeId;
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_ui_graphics::Size as ViewportSize;

use super::*;

const ROWS: usize = 40;
const ROW_HEIGHT: f32 = 48.0;
const VIEWPORT: ViewportSize = ViewportSize {
    width: 320.0,
    height: 200.0,
};

fn measure(composition: &mut TestComposition, root: NodeId) {
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    let layout = applier
        .compute_layout(root, VIEWPORT)
        .expect("layout measurement");
    applier.clear_runtime_handle();
    drop(applier);
    HeadlessRenderer::new().render(&layout);
    composition.runtime_handle().drain_ui();
    composition
        .process_invalid_scopes()
        .expect("draw subscription recomposition");
}

#[composable]
#[allow(non_snake_case)]
fn ParkedRow(index: usize, nested: bool) {
    let body = move || {
        if index == 0 {
            cranpose_core::LaunchedEffectAsync!(0u32, move |_scope| {
                Box::pin(async move {
                    std::future::pending::<()>().await;
                })
            });
        }
        Text(
            format!("row {index}"),
            Modifier::empty().height(ROW_HEIGHT),
            TextStyle::default(),
        );
    };
    match nested {
        true => {
            SwipeToDismiss(
                Modifier::empty().fill_max_width(),
                SwipeToDismissSpec::default(),
                || {},
                body,
            );
        }
        false => body(),
    }
}

fn task_counts_across_reuse(nested: bool, scroll_back: bool) -> Vec<usize> {
    let held: Rc<RefCell<Option<LazyListState>>> = Rc::new(RefCell::new(None));
    let keeper = Rc::clone(&held);
    let mut composition = run_test_composition(move || {
        let list_state = rememberLazyListState();
        *keeper.borrow_mut() = Some(list_state);
        LazyColumn(
            Modifier::empty().fill_max_size(),
            list_state,
            LazyColumnSpec::default(),
            move |scope| {
                scope.items(
                    LazyItems::new(ROWS).key(|index: usize| index as u64),
                    move |index| ParkedRow(index, nested),
                );
            },
        );
    });

    let root = composition.root().expect("root node");
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    let mut counts = vec![composition.runtime_handle().debug_stats().tasks_len];

    let list_state = (*held.borrow()).expect("list state");
    list_state.scroll_to_item(ROWS - 4, 0.0);
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    counts.push(composition.runtime_handle().debug_stats().tasks_len);

    if scroll_back {
        list_state.scroll_to_item(0, 0.0);
        measure(&mut composition, root);
        composition.runtime_handle().drain_ui();
        counts.push(composition.runtime_handle().debug_stats().tasks_len);
    }
    counts
}

#[test]
fn a_recycled_lazy_row_stops_its_parked_effect() {
    assert_eq!(
        task_counts_across_reuse(false, false),
        vec![1, 0],
        "the row's effect must run on screen and be cancelled off screen"
    );
}

#[test]
fn a_recycled_lazy_row_stops_the_effect_of_a_widget_one_subcompose_deeper() {
    assert_eq!(
        task_counts_across_reuse(true, false),
        vec![1, 0],
        "the SwipeToDismiss effect must run on screen and be cancelled off screen"
    );
}

struct DropMark(Rc<Cell<usize>>);

impl Drop for DropMark {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

#[composable]
#[allow(non_snake_case)]
fn MarkGroup(dropped: Rc<Cell<usize>>) {
    cranpose_core::remember(move || DropMark(dropped));
}

#[composable]
#[allow(non_snake_case)]
fn BusyRow(index: usize, state: cranpose_core::MutableState<bool>, dropped: Rc<Cell<usize>>) {
    let busy = state.get() && index == 0;
    if busy {
        cranpose_core::remember({
            let dropped = Rc::clone(&dropped);
            move || DropMark(dropped)
        });
        MarkGroup(dropped);
        cranpose_core::LaunchedEffectAsync!(0u32, move |_scope| {
            Box::pin(async move {
                std::future::pending::<()>().await;
            })
        });
        Text(
            format!("row {index} busy"),
            Modifier::empty().height(ROW_HEIGHT),
            TextStyle::default(),
        );
    } else {
        Text(
            format!("row {index} done"),
            Modifier::empty().height(ROW_HEIGHT),
            TextStyle::default(),
        );
    }
}

#[composable]
#[allow(non_snake_case)]
fn BadgeRow(index: usize, state: cranpose_core::MutableState<bool>) {
    use cranpose_animation::{
        AnimationSpec, Easing, RepeatMode, StartOffset, infiniteRepeatable,
        rememberInfiniteTransition,
    };
    let busy = state.get() && index == 0;
    Column(Modifier::empty(), ColumnSpec::new(), move || {
        Text(
            format!("row {index}"),
            Modifier::empty().height(ROW_HEIGHT),
            TextStyle::default(),
        );
        if busy {
            Row(Modifier::empty(), RowSpec::new(), move || {
                crate::widgets::CircularProgressIndicator(
                    Modifier::empty(),
                    cranpose_ui_graphics::Color(0.2, 0.5, 0.9, 1.0),
                    1.8,
                );
                let pulse = rememberInfiniteTransition("recognizing").animateFloat(
                    0.45,
                    1.0,
                    infiniteRepeatable(
                        AnimationSpec::tween(900, Easing::EaseInOut),
                        RepeatMode::Reverse,
                        StartOffset::default(),
                    ),
                    "alpha",
                );
                Text(
                    format!("reading {:.2}", pulse.get()),
                    Modifier::empty().height(ROW_HEIGHT),
                    TextStyle::default(),
                );
            });
        }
    });
}

fn badge_task_counts_across_swap(nested: bool) -> Vec<usize> {
    let held: Rc<RefCell<Option<cranpose_core::MutableState<bool>>>> = Rc::new(RefCell::new(None));
    let keeper = Rc::clone(&held);
    let mut composition = run_test_composition(move || {
        let busy = cranpose_core::rememberMutableStateOf(|| true);
        *keeper.borrow_mut() = Some(busy);
        match nested {
            true => {
                let list_state = rememberLazyListState();
                LazyColumn(
                    Modifier::empty().fill_max_size(),
                    list_state,
                    LazyColumnSpec::default(),
                    move |scope| {
                        scope.items(
                            LazyItems::new(ROWS).key(|index: usize| index as u64),
                            move |index| BadgeRow(index, busy),
                        );
                    },
                );
            }
            false => BadgeRow(0, busy),
        }
    });

    let root = composition.root().expect("root node");
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    let mut counts = vec![composition.runtime_handle().debug_stats().tasks_len];

    let busy = (*held.borrow()).expect("busy state");
    busy.set_value(false);
    composition
        .process_invalid_scopes()
        .expect("the row stops being busy");
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    counts.push(composition.runtime_handle().debug_stats().tasks_len);
    counts
}

#[test]
fn a_plain_badge_that_leaves_the_screen_stops_its_loops() {
    assert_eq!(
        badge_task_counts_across_swap(false),
        vec![2, 0],
        "the badge loops must stop when the badge leaves"
    );
}

#[test]
fn a_lazy_badge_that_leaves_the_screen_stops_its_loops() {
    assert_eq!(
        badge_task_counts_across_swap(true),
        vec![2, 0],
        "the badge loops in a lazy row must stop when the badge leaves"
    );
}

#[composable]
#[allow(non_snake_case)]
fn CapturedBusyRow(index: usize, busy: bool) {
    if busy {
        cranpose_core::LaunchedEffectAsync!(0u32, move |_scope| {
            Box::pin(async move {
                std::future::pending::<()>().await;
            })
        });
        Text(
            format!("row {index} busy"),
            Modifier::empty().height(ROW_HEIGHT),
            TextStyle::default(),
        );
    } else {
        Text(
            format!("row {index} done"),
            Modifier::empty().height(ROW_HEIGHT),
            TextStyle::default(),
        );
    }
}

#[composable]
#[allow(non_snake_case)]
fn CapturedList(state: cranpose_core::MutableState<bool>) {
    let busy_now = state.get();
    let list_state = rememberLazyListState();
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list_state,
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(
                LazyItems::new(ROWS).key(|index: usize| index as u64),
                move |index| CapturedBusyRow(index, busy_now && index == 0),
            );
        },
    );
}

fn task_counts_across_captured_swap() -> Vec<usize> {
    let held: Rc<RefCell<Option<cranpose_core::MutableState<bool>>>> = Rc::new(RefCell::new(None));
    let keeper = Rc::clone(&held);
    let mut composition = run_test_composition(move || {
        let busy = cranpose_core::rememberMutableStateOf(|| true);
        *keeper.borrow_mut() = Some(busy);
        CapturedList(busy);
    });

    let root = composition.root().expect("root node");
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    let mut counts = vec![composition.runtime_handle().debug_stats().tasks_len];

    let busy = (*held.borrow()).expect("busy state");
    busy.set_value(false);
    while composition
        .process_invalid_scopes()
        .expect("the list stops being busy")
    {}
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    counts.push(composition.runtime_handle().debug_stats().tasks_len);
    counts
}

#[composable]
#[allow(non_snake_case)]
fn CapturedBadgeRow(index: usize, busy: bool) {
    use cranpose_animation::{
        AnimationSpec, Easing, RepeatMode, StartOffset, infiniteRepeatable,
        rememberInfiniteTransition,
    };
    Column(Modifier::empty(), ColumnSpec::new(), move || {
        Text(
            format!("row {index}"),
            Modifier::empty().height(ROW_HEIGHT),
            TextStyle::default(),
        );
        if busy {
            Row(Modifier::empty(), RowSpec::new(), move || {
                crate::widgets::CircularProgressIndicator(
                    Modifier::empty(),
                    cranpose_ui_graphics::Color(0.2, 0.5, 0.9, 1.0),
                    1.8,
                );
                let pulse = rememberInfiniteTransition("recognizing").animateFloat(
                    0.45,
                    1.0,
                    infiniteRepeatable(
                        AnimationSpec::tween(900, Easing::EaseInOut),
                        RepeatMode::Reverse,
                        StartOffset::default(),
                    ),
                    "alpha",
                );
                Text(
                    format!("reading {:.2}", pulse.get()),
                    Modifier::empty().height(ROW_HEIGHT),
                    TextStyle::default(),
                );
            });
        }
    });
}

#[composable]
#[allow(non_snake_case)]
fn CapturedBadgeItemList(state: cranpose_core::MutableState<bool>) {
    use cranpose_animation::{
        AnimationSpec, Easing, RepeatMode, StartOffset, infiniteRepeatable,
        rememberInfiniteTransition,
    };
    let busy_now = state.get();
    let list_state = rememberLazyListState();
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list_state,
        LazyColumnSpec::default(),
        move |scope| {
            scope.item_keyed(Some(1), None, move || {
                Text(
                    "header".to_string(),
                    Modifier::empty().height(ROW_HEIGHT),
                    TextStyle::default(),
                );
                if busy_now {
                    Row(Modifier::empty(), RowSpec::new(), move || {
                        crate::widgets::CircularProgressIndicator(
                            Modifier::empty(),
                            cranpose_ui_graphics::Color(0.2, 0.5, 0.9, 1.0),
                            1.8,
                        );
                        let pulse = rememberInfiniteTransition("recognizing").animateFloat(
                            0.45,
                            1.0,
                            infiniteRepeatable(
                                AnimationSpec::tween(900, Easing::EaseInOut),
                                RepeatMode::Reverse,
                                StartOffset::default(),
                            ),
                            "alpha",
                        );
                        Text(
                            format!("reading {:.2}", pulse.get()),
                            Modifier::empty().height(ROW_HEIGHT),
                            TextStyle::default(),
                        );
                    });
                }
            });
            scope.items(
                LazyItems::new(ROWS).key(|index: usize| index as u64 + 100),
                move |index| {
                    Text(
                        format!("row {index}"),
                        Modifier::empty().height(ROW_HEIGHT),
                        TextStyle::default(),
                    );
                },
            );
        },
    );
}

fn badge_task_counts_across_item_swap() -> Vec<usize> {
    let held: Rc<RefCell<Option<cranpose_core::MutableState<bool>>>> = Rc::new(RefCell::new(None));
    let keeper = Rc::clone(&held);
    let mut composition = run_test_composition(move || {
        let busy = cranpose_core::rememberMutableStateOf(|| true);
        *keeper.borrow_mut() = Some(busy);
        CapturedBadgeItemList(busy);
    });

    let root = composition.root().expect("root node");
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    let mut counts = vec![composition.runtime_handle().debug_stats().tasks_len];

    let busy = (*held.borrow()).expect("busy state");
    busy.set_value(false);
    while composition
        .process_invalid_scopes()
        .expect("the header stops being busy")
    {}
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    counts.push(composition.runtime_handle().debug_stats().tasks_len);
    counts
}

#[test]
fn a_badge_in_a_lazy_header_item_stops_its_loops_when_it_goes() {
    assert_eq!(
        badge_task_counts_across_item_swap(),
        vec![2, 0],
        "the badge loops in a header item must stop when the header drops the badge"
    );
}

#[composable]
#[allow(non_snake_case)]
fn ToastHost(state: cranpose_core::MutableState<bool>) {
    use cranpose_animation::{
        AnimationSpec, Easing, RepeatMode, StartOffset, infiniteRepeatable,
        rememberInfiniteTransition,
    };
    let shown = state.get();
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::new(),
        move || {
            Text(
                "screen".to_string(),
                Modifier::empty().height(ROW_HEIGHT),
                TextStyle::default(),
            );
            crate::widgets::AnimatedVisibility(
                shown,
                crate::fade_in().with_animation(cranpose_animation::tween(
                    180,
                    cranpose_animation::Easing::EaseOut,
                )),
                crate::fade_out().with_animation(cranpose_animation::tween(
                    140,
                    cranpose_animation::Easing::EaseIn,
                )),
                move || {
                    Row(Modifier::empty(), RowSpec::new(), move || {
                        crate::widgets::CircularProgressIndicator(
                            Modifier::empty(),
                            cranpose_ui_graphics::Color(0.2, 0.5, 0.9, 1.0),
                            1.8,
                        );
                        let pulse = rememberInfiniteTransition("recognizing").animateFloat(
                            0.45,
                            1.0,
                            infiniteRepeatable(
                                AnimationSpec::tween(900, Easing::EaseInOut),
                                RepeatMode::Reverse,
                                StartOffset::default(),
                            ),
                            "alpha",
                        );
                        Text(
                            format!("reading {:.2}", pulse.get()),
                            Modifier::empty().height(ROW_HEIGHT),
                            TextStyle::default(),
                        );
                    });
                },
            );
        },
    );
}

fn toast_task_counts_across_shows(shows: usize) -> Vec<usize> {
    let held: Rc<RefCell<Option<cranpose_core::MutableState<bool>>>> = Rc::new(RefCell::new(None));
    let keeper = Rc::clone(&held);
    let mut composition = run_test_composition(move || {
        let shown = cranpose_core::rememberMutableStateOf(|| true);
        *keeper.borrow_mut() = Some(shown);
        ToastHost(shown);
    });

    let root = composition.root().expect("root node");
    let mut clock = 0u64;
    let step = |composition: &mut TestComposition, clock: &mut u64| {
        for _ in 0..12 {
            *clock += 100_000_000;
            let runtime = composition.runtime_handle();
            runtime.drain_frame_callbacks(*clock);
            runtime.drain_ui();
            while composition
                .process_invalid_scopes()
                .expect("the toast settles")
            {}
        }
        measure(composition, root);
        composition.runtime_handle().drain_ui();
    };

    let shown = {
        step(&mut composition, &mut clock);
        (*held.borrow()).expect("toast state")
    };
    let mut counts = Vec::new();
    for _ in 0..shows {
        shown.set_value(false);
        step(&mut composition, &mut clock);
        counts.push(composition.runtime_handle().debug_stats().tasks_len);
        shown.set_value(true);
        step(&mut composition, &mut clock);
    }
    counts
}

#[test]
fn a_toast_that_fades_out_stops_the_loops_it_showed() {
    assert_eq!(
        toast_task_counts_across_shows(3),
        vec![0, 0, 0],
        "every toast must take its loops with it when it fades out"
    );
}

#[composable]
#[allow(non_snake_case)]
fn ShiftingBadgeList(state: cranpose_core::MutableState<bool>) {
    use cranpose_animation::{
        AnimationSpec, Easing, RepeatMode, StartOffset, infiniteRepeatable,
        rememberInfiniteTransition,
    };
    let banner = state.get();
    let list_state = rememberLazyListState();
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list_state,
        LazyColumnSpec::default(),
        move |scope| {
            if banner {
                scope.item(move || {
                    Text(
                        "reading".to_string(),
                        Modifier::empty().height(ROW_HEIGHT),
                        TextStyle::default(),
                    );
                });
            }
            scope.item(move || {
                Row(Modifier::empty(), RowSpec::new(), move || {
                    crate::widgets::CircularProgressIndicator(
                        Modifier::empty(),
                        cranpose_ui_graphics::Color(0.2, 0.5, 0.9, 1.0),
                        1.8,
                    );
                    let pulse = rememberInfiniteTransition("recognizing").animateFloat(
                        0.45,
                        1.0,
                        infiniteRepeatable(
                            AnimationSpec::tween(900, Easing::EaseInOut),
                            RepeatMode::Reverse,
                            StartOffset::default(),
                        ),
                        "alpha",
                    );
                    Text(
                        format!("reading {:.2}", pulse.get()),
                        Modifier::empty().height(ROW_HEIGHT),
                        TextStyle::default(),
                    );
                });
            });
            scope.items(
                LazyItems::new(ROWS).key(|index: usize| index as u64 + 100),
                move |index| {
                    Text(
                        format!("row {index}"),
                        Modifier::empty().height(ROW_HEIGHT),
                        TextStyle::default(),
                    );
                },
            );
        },
    );
}

fn badge_task_counts_across_shifts(shifts: usize) -> Vec<usize> {
    let held: Rc<RefCell<Option<cranpose_core::MutableState<bool>>>> = Rc::new(RefCell::new(None));
    let keeper = Rc::clone(&held);
    let mut composition = run_test_composition(move || {
        let banner = cranpose_core::rememberMutableStateOf(|| false);
        *keeper.borrow_mut() = Some(banner);
        ShiftingBadgeList(banner);
    });

    let root = composition.root().expect("root node");
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    let mut counts = vec![composition.runtime_handle().debug_stats().tasks_len];

    let banner = (*held.borrow()).expect("banner state");
    for step in 0..shifts {
        banner.set_value(step % 2 == 0);
        while composition
            .process_invalid_scopes()
            .expect("the banner comes and goes")
        {}
        measure(&mut composition, root);
        composition.runtime_handle().drain_ui();
        counts.push(composition.runtime_handle().debug_stats().tasks_len);
    }
    counts
}

#[test]
fn a_badge_keeps_one_pair_of_loops_while_items_shift_around_it() {
    assert_eq!(
        badge_task_counts_across_shifts(4),
        vec![2, 2, 2, 2, 2],
        "an item that appears above the badge must not leave the badge loops behind"
    );
}

#[composable]
#[allow(non_snake_case)]
fn CapturedBadgeList(state: cranpose_core::MutableState<bool>) {
    let busy_now = state.get();
    let list_state = rememberLazyListState();
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list_state,
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(
                LazyItems::new(ROWS).key(|index: usize| index as u64),
                move |index| CapturedBadgeRow(index, busy_now && index == 0),
            );
        },
    );
}

fn badge_task_counts_across_captured_swap() -> Vec<usize> {
    let held: Rc<RefCell<Option<cranpose_core::MutableState<bool>>>> = Rc::new(RefCell::new(None));
    let keeper = Rc::clone(&held);
    let mut composition = run_test_composition(move || {
        let busy = cranpose_core::rememberMutableStateOf(|| true);
        *keeper.borrow_mut() = Some(busy);
        CapturedBadgeList(busy);
    });

    let root = composition.root().expect("root node");
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    let mut counts = vec![composition.runtime_handle().debug_stats().tasks_len];

    let busy = (*held.borrow()).expect("busy state");
    busy.set_value(false);
    while composition
        .process_invalid_scopes()
        .expect("the list stops being busy")
    {}
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    counts.push(composition.runtime_handle().debug_stats().tasks_len);
    counts
}

#[test]
fn a_badge_in_a_lazy_row_stops_its_loops_when_its_parent_drops_it() {
    assert_eq!(
        badge_task_counts_across_captured_swap(),
        vec![2, 0],
        "the badge loops must stop when the parent stops asking for the badge"
    );
}

#[test]
fn a_lazy_row_follows_a_value_its_parent_captured() {
    assert_eq!(
        task_counts_across_captured_swap(),
        vec![1, 0],
        "a row must take the new value its parent captured for it"
    );
}

struct SwapSample {
    tasks: usize,
    drops: usize,
}

fn samples_across_swap(nested: bool) -> Vec<SwapSample> {
    let held: Rc<RefCell<Option<cranpose_core::MutableState<bool>>>> = Rc::new(RefCell::new(None));
    let keeper = Rc::clone(&held);
    let dropped = Rc::new(Cell::new(0));
    let mut composition = run_test_composition({
        let dropped = Rc::clone(&dropped);
        move || {
            let busy = cranpose_core::rememberMutableStateOf(|| true);
            *keeper.borrow_mut() = Some(busy);
            match nested {
                true => {
                    let list_state = rememberLazyListState();
                    let dropped = Rc::clone(&dropped);
                    LazyColumn(
                        Modifier::empty().fill_max_size(),
                        list_state,
                        LazyColumnSpec::default(),
                        move |scope| {
                            scope.items(
                                LazyItems::new(ROWS).key(|index: usize| index as u64),
                                move |index| BusyRow(index, busy, Rc::clone(&dropped)),
                            );
                        },
                    );
                }
                false => BusyRow(0, busy, Rc::clone(&dropped)),
            }
        }
    });

    let root = composition.root().expect("root node");
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    let mut samples = vec![SwapSample {
        tasks: composition.runtime_handle().debug_stats().tasks_len,
        drops: dropped.get(),
    }];

    let busy = (*held.borrow()).expect("busy state");
    busy.set_value(false);
    composition
        .process_invalid_scopes()
        .expect("the row stops being busy");
    let root = composition.root().expect("root node after the swap");
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    samples.push(SwapSample {
        tasks: composition.runtime_handle().debug_stats().tasks_len,
        drops: dropped.get(),
    });
    samples
}

fn task_counts_across_swap(nested: bool) -> Vec<usize> {
    samples_across_swap(nested)
        .iter()
        .map(|sample| sample.tasks)
        .collect()
}

fn drop_counts_across_swap(nested: bool) -> Vec<usize> {
    samples_across_swap(nested)
        .iter()
        .map(|sample| sample.drops)
        .collect()
}

#[test]
fn a_plain_row_that_swaps_its_content_stops_the_effect_it_dropped() {
    assert_eq!(
        task_counts_across_swap(false),
        vec![1, 0],
        "outside a lazy list the dropped branch must stop its effect"
    );
}

#[test]
fn a_plain_row_that_swaps_its_content_frees_the_slots_it_dropped() {
    assert_eq!(
        drop_counts_across_swap(false),
        vec![0, 2],
        "the remembered values of the dropped branch must be freed at the swap"
    );
}

#[test]
fn a_lazy_row_that_swaps_its_content_stops_the_effect_it_dropped() {
    assert_eq!(
        task_counts_across_swap(true),
        vec![1, 0],
        "a row that stops being busy must stop the effect of the branch it dropped"
    );
}

#[test]
fn a_lazy_row_that_swaps_its_content_frees_the_slots_it_dropped() {
    assert_eq!(
        drop_counts_across_swap(true),
        vec![0, 2],
        "the remembered values of the dropped row branch must be freed at the swap"
    );
}

#[composable]
#[allow(non_snake_case)]
fn BadgeListScreen(state: cranpose_core::MutableState<bool>) {
    let list_state = rememberLazyListState();
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list_state,
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(
                LazyItems::new(ROWS).key(|index: usize| index as u64),
                move |index| {
                    SwipeToDismiss(
                        Modifier::empty().fill_max_width(),
                        SwipeToDismissSpec::default().with_key(index as u64),
                        || {},
                        move || BadgeRow(index, state),
                    );
                },
            );
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn BadgeSwipeScreen(state: cranpose_core::MutableState<bool>) {
    SwipeToDismiss(
        Modifier::empty().fill_max_width(),
        SwipeToDismissSpec::default().with_key(0),
        || {},
        move || BadgeRow(0, state),
    );
}

#[composable]
#[allow(non_snake_case)]
fn BadgeConstraintsScreen(state: cranpose_core::MutableState<bool>) {
    crate::widgets::BoxWithConstraints(Modifier::empty().fill_max_width(), move |_scope| {
        BadgeRow(0, state);
    });
}

#[composable]
#[allow(non_snake_case)]
fn BadgeTwoConstraintsScreen(state: cranpose_core::MutableState<bool>) {
    crate::widgets::BoxWithConstraints(Modifier::empty().fill_max_width(), move |_outer| {
        crate::widgets::BoxWithConstraints(Modifier::empty().fill_max_width(), move |_inner| {
            BadgeRow(0, state);
        });
    });
}

#[composable]
#[allow(non_snake_case)]
fn SwipeHost(
    on_row: cranpose_core::MutableState<bool>,
    busy: cranpose_core::MutableState<bool>,
    shape: usize,
) {
    match (on_row.get(), shape) {
        (true, 0) => BadgeSwipeScreen(busy),
        (true, 1) => BadgeConstraintsScreen(busy),
        (true, _) => BadgeTwoConstraintsScreen(busy),
        (false, _) => PlainScreen(),
    }
}

fn task_counts_across_constraints_swap() -> Vec<usize> {
    task_counts_across_subcompose_swap(1)
}

fn task_counts_across_two_constraints_swap() -> Vec<usize> {
    task_counts_across_subcompose_swap(2)
}

fn task_counts_across_swipe_swap() -> Vec<usize> {
    task_counts_across_subcompose_swap(0)
}

fn task_counts_across_subcompose_swap(shape: usize) -> Vec<usize> {
    let held: Rc<RefCell<Option<cranpose_core::MutableState<bool>>>> = Rc::new(RefCell::new(None));
    let keeper = Rc::clone(&held);
    let mut composition = run_test_composition(move || {
        let on_row = cranpose_core::rememberMutableStateOf(|| true);
        let busy = cranpose_core::rememberMutableStateOf(|| true);
        *keeper.borrow_mut() = Some(on_row);
        SwipeHost(on_row, busy, shape);
    });

    let root = composition.root().expect("root node");
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    let mut counts = vec![composition.runtime_handle().debug_stats().tasks_len];

    let on_row = (*held.borrow()).expect("row state");
    on_row.set_value(false);
    composition
        .process_invalid_scopes()
        .expect("the row leaves the screen");
    let root = composition.root().expect("root node after the swap");
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    counts.push(composition.runtime_handle().debug_stats().tasks_len);
    counts
}

#[test]
fn a_swipe_row_that_leaves_the_screen_stops_the_loops_it_showed() {
    assert_eq!(
        task_counts_across_swipe_swap(),
        vec![2, 0],
        "the loops inside a swipe row must stop when the row leaves the composition"
    );
}

#[test]
fn a_box_with_constraints_that_leaves_the_screen_stops_the_loops_it_showed() {
    assert_eq!(
        task_counts_across_constraints_swap(),
        vec![2, 0],
        "the loops inside a BoxWithConstraints must stop when it leaves the composition"
    );
}

#[test]
fn two_nested_boxes_with_constraints_stop_the_loops_they_showed() {
    assert_eq!(
        task_counts_across_two_constraints_swap(),
        vec![2, 0],
        "one subcompose inside another must still stop its loops when both leave"
    );
}

#[composable]
#[allow(non_snake_case)]
fn PlainScreen() {
    Text(
        "another screen".to_string(),
        Modifier::empty().height(ROW_HEIGHT),
        TextStyle::default(),
    );
}

#[composable]
#[allow(non_snake_case)]
fn ScreenHost(on_list: cranpose_core::MutableState<bool>, busy: cranpose_core::MutableState<bool>) {
    let on_list_now = on_list.get();
    crate::widgets::Box(
        Modifier::empty().fill_max_size(),
        crate::widgets::BoxSpec::default(),
        move || {
            crate::widgets::Box(
                Modifier::empty().fill_max_size(),
                crate::widgets::BoxSpec::default(),
                move || match on_list_now {
                    true => BadgeListScreen(busy),
                    false => PlainScreen(),
                },
            );
            Text(
                "tab bar".to_string(),
                Modifier::empty().height(ROW_HEIGHT),
                TextStyle::default(),
            );
        },
    );
}

fn task_counts_across_screen_swaps(rounds: usize) -> Vec<usize> {
    type Held = Rc<
        RefCell<
            Option<(
                cranpose_core::MutableState<bool>,
                cranpose_core::MutableState<bool>,
            )>,
        >,
    >;
    let held: Held = Rc::new(RefCell::new(None));
    let keeper = Rc::clone(&held);
    let mut composition = run_test_composition(move || {
        let on_list = cranpose_core::rememberMutableStateOf(|| true);
        let busy = cranpose_core::rememberMutableStateOf(|| true);
        *keeper.borrow_mut() = Some((on_list, busy));
        ScreenHost(on_list, busy);
    });

    let root = composition.root().expect("root node");
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    let mut counts = vec![composition.runtime_handle().debug_stats().tasks_len];

    let (on_list, _busy) = (*held.borrow()).expect("screen state");
    for round in 0..rounds {
        on_list.set_value(round % 2 == 1);
        composition
            .process_invalid_scopes()
            .expect("the screen swaps");
        let root = composition.root().expect("root node after the swap");
        measure(&mut composition, root);
        composition.runtime_handle().drain_ui();
        counts.push(composition.runtime_handle().debug_stats().tasks_len);
    }
    counts
}

#[test]
fn a_lazy_list_that_leaves_the_screen_stops_the_loops_of_its_rows() {
    assert_eq!(
        task_counts_across_screen_swaps(1),
        vec![2, 0],
        "the badge loops must stop when the whole list leaves the composition"
    );
}

#[test]
fn a_lazy_list_that_comes_back_keeps_one_pair_of_loops() {
    assert_eq!(
        task_counts_across_screen_swaps(4),
        vec![2, 0, 2, 0, 2],
        "every return to the list must hold one pair of loops, not add one"
    );
}

#[composable]
#[allow(non_snake_case)]
fn EarlyReturnBanner(count: usize) {
    use cranpose_animation::{
        AnimationSpec, Easing, RepeatMode, StartOffset, infiniteRepeatable,
        rememberInfiniteTransition,
    };
    if count == 0 {
        return;
    }
    Row(Modifier::empty(), RowSpec::new(), move || {
        crate::widgets::CircularProgressIndicator(
            Modifier::empty(),
            cranpose_ui_graphics::Color(0.2, 0.5, 0.9, 1.0),
            1.8,
        );
        let pulse = rememberInfiniteTransition("banner").animateFloat(
            0.45,
            1.0,
            infiniteRepeatable(
                AnimationSpec::tween(900, Easing::EaseInOut),
                RepeatMode::Reverse,
                StartOffset::default(),
            ),
            "alpha",
        );
        Text(
            format!("processing {count} at {:.2}", pulse.get()),
            Modifier::empty().height(ROW_HEIGHT),
            TextStyle::default(),
        );
    });
}

#[composable]
#[allow(non_snake_case)]
fn BannerHost(state: cranpose_core::MutableState<usize>) {
    let count = state.get();
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::new(),
        move || {
            EarlyReturnBanner(count);
            Text(
                "library".to_string(),
                Modifier::empty().height(ROW_HEIGHT),
                TextStyle::default(),
            );
        },
    );
}

fn banner_task_counts_across_finish() -> Vec<usize> {
    let held: Rc<RefCell<Option<cranpose_core::MutableState<usize>>>> = Rc::new(RefCell::new(None));
    let keeper = Rc::clone(&held);
    let mut composition = run_test_composition(move || {
        let count = cranpose_core::rememberMutableStateOf(|| 1usize);
        *keeper.borrow_mut() = Some(count);
        BannerHost(count);
    });

    let root = composition.root().expect("root node");
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    let mut counts = vec![composition.runtime_handle().debug_stats().tasks_len];

    let count = (*held.borrow()).expect("count state");
    count.set_value(0);
    composition
        .process_invalid_scopes()
        .expect("the banner has nothing left to show");
    measure(&mut composition, root);
    composition.runtime_handle().drain_ui();
    counts.push(composition.runtime_handle().debug_stats().tasks_len);
    counts
}

#[test]
fn a_banner_that_returns_early_stops_the_loops_it_showed() {
    assert_eq!(
        banner_task_counts_across_finish(),
        vec![2, 0],
        "a composable that returns before it emits anything must stop the loops of its last pass"
    );
}

#[test]
fn a_recycled_lazy_row_restarts_its_effect_when_it_returns() {
    assert_eq!(
        (
            task_counts_across_reuse(false, true),
            task_counts_across_reuse(true, true),
        ),
        (vec![1, 0, 1], vec![1, 0, 1]),
        "reusing a row must restart its nested effect exactly once"
    );
}
