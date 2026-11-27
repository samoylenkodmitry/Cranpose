use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{MutableState, SnapshotStateObserver, key, remember, rememberMutableStateOf};
use cranpose_foundation::{PointerButton, PointerButtons, PointerEvent, PointerEventKind};
use cranpose_ui::{
    Axis, Box, BoxSpec, Column, ColumnSpec, DraggableState, MeasureLayoutOptions, Modifier,
    MutableInteractionSource, Point, ScrollState, Size, Spacer, TestComposition,
    build_layout_tree_from_applier, build_semantics_tree_from_applier, measure_layout,
    measure_layout_with_options, rememberDraggableState, rememberMutableInteractionSource,
    round_scaling_list::CentreAnchor,
    run_test_composition,
    widgets::wear::scaling_list::{
        WearScalingLazyColumn, WearScalingLazyColumnSpec, WearScalingListState,
        rememberWearScalingListState,
    },
};

const VIEWPORT: Size = Size {
    width: 320.0,
    height: 240.0,
};

fn layout_and_semantics(composition: &mut TestComposition) {
    let root = composition.root().expect("composed root");
    composition.with_runtime_applier(|applier| {
        measure_layout(applier, root, VIEWPORT).expect("layout succeeds");
        build_semantics_tree_from_applier(applier, root).expect("semantics tree builds");
    });
}

fn settle_composition(composition: &mut TestComposition) {
    while composition
        .process_invalid_scopes()
        .expect("recompose succeeds")
    {}
    layout_and_semantics(composition);
}

fn pointer_handlers(composition: &mut TestComposition) -> Vec<Rc<dyn Fn(PointerEvent)>> {
    let root = composition.root().expect("composed root");
    composition.with_runtime_applier(|applier| {
        let tree = build_layout_tree_from_applier(applier, root)
            .expect("layout tree build")
            .expect("layout tree root");
        let mut collected = Vec::new();
        let mut stack = vec![tree.root()];
        while let Some(node) = stack.pop() {
            collected.extend(
                node.node_data
                    .modifier_slices()
                    .pointer_inputs()
                    .iter()
                    .cloned(),
            );
            for child in &node.children {
                stack.push(child);
            }
        }
        collected
    })
}

struct SplitOwnerHarness {
    composition: TestComposition,
    keep_owner: MutableState<bool>,
    keep_node: MutableState<bool>,
}

impl SplitOwnerHarness {
    fn new<T: Clone + 'static>(
        owner_key: u64,
        node_key: u64,
        build_owner: impl Fn() -> T + 'static,
        build_node: impl Fn(T) + 'static,
    ) -> Self {
        let keep_owner_slot: Rc<Cell<Option<MutableState<bool>>>> = Rc::new(Cell::new(None));
        let keep_node_slot: Rc<Cell<Option<MutableState<bool>>>> = Rc::new(Cell::new(None));
        let shared: Rc<RefCell<Option<T>>> = Rc::new(RefCell::new(None));

        let keep_owner_cap = Rc::clone(&keep_owner_slot);
        let keep_node_cap = Rc::clone(&keep_node_slot);
        let shared_cap = Rc::clone(&shared);
        let build_owner = Rc::new(build_owner);
        let build_node = Rc::new(build_node);

        let mut composition = run_test_composition(move || {
            let keep_owner = rememberMutableStateOf(|| true);
            let keep_node = rememberMutableStateOf(|| true);
            keep_owner_cap.set(Some(keep_owner));
            keep_node_cap.set(Some(keep_node));

            let shared_owner = Rc::clone(&shared_cap);
            let shared_consumer = Rc::clone(&shared_cap);
            let build_owner = Rc::clone(&build_owner);
            let build_node = Rc::clone(&build_node);
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::default(),
                move || {
                    if keep_owner.get() {
                        let shared_owner = Rc::clone(&shared_owner);
                        let build_owner = Rc::clone(&build_owner);
                        key(owner_key, move || {
                            *shared_owner.borrow_mut() = Some(build_owner());
                        });
                    }
                    if keep_node.get() {
                        let shared_consumer = Rc::clone(&shared_consumer);
                        let build_node = Rc::clone(&build_node);
                        key(node_key, move || {
                            if let Some(state) = shared_consumer.borrow().clone() {
                                build_node(state);
                            }
                        });
                    }
                },
            );
        });

        layout_and_semantics(&mut composition);
        Self {
            composition,
            keep_owner: keep_owner_slot.get().expect("keep_owner"),
            keep_node: keep_node_slot.get().expect("keep_node"),
        }
    }

    fn dispose_owner(&mut self) {
        self.keep_owner.set(false);
        settle_composition(&mut self.composition);
    }

    fn detach_node(&mut self) {
        self.keep_node.set(false);
        settle_composition(&mut self.composition);
    }
}

fn scrollable_column(state: ScrollState) {
    Column(
        Modifier::empty()
            .fill_max_width()
            .height(100.0)
            .vertical_scroll(state, false),
        ColumnSpec::default(),
        || {
            for _ in 0..10 {
                Spacer(Modifier::empty().fill_max_width().height(24.0));
            }
        },
    );
}

fn run_animation_past_owner_disposal<S: Copy + 'static>(
    build_content: impl Fn() -> S + 'static,
    start_animation: impl FnOnce(S),
) {
    let show_slot: Rc<Cell<Option<MutableState<bool>>>> = Rc::new(Cell::new(None));
    let state_slot: Rc<Cell<Option<S>>> = Rc::new(Cell::new(None));

    let show_cap = Rc::clone(&show_slot);
    let state_cap = Rc::clone(&state_slot);
    let build_content = Rc::new(build_content);

    let mut composition = run_test_composition(move || {
        let show = rememberMutableStateOf(|| true);
        show_cap.set(Some(show));
        let state_cap = Rc::clone(&state_cap);
        let build_content = Rc::clone(&build_content);
        Box(
            Modifier::empty().fill_max_size(),
            BoxSpec::default(),
            move || {
                if show.get() {
                    state_cap.set(Some(build_content()));
                }
            },
        );
    });

    layout_and_semantics(&mut composition);
    let state = state_slot.get().expect("animated state");
    let show = show_slot.get().expect("show switch");
    let handle = composition.runtime_handle();

    composition.with_app_context(|| {
        start_animation(state);
    });
    composition.with_app_context(|| {
        handle.drain_frame_callbacks(16_000_000);
    });

    show.set(false);
    settle_composition(&mut composition);

    composition.with_app_context(|| {
        handle.drain_frame_callbacks(32_000_000);
    });
}

#[test]
fn scroll_node_detaches_and_measures_after_scroll_state_owner_disposes() {
    let mut harness = SplitOwnerHarness::new(
        2001,
        2002,
        || {
            let owned = remember(|| ScrollState::new(12.0)).with(|state| *state);
            // A sibling box forces a layout pass on the remaining node when removed.
            Spacer(Modifier::empty().size_points(10.0, 10.0));
            owned
        },
        scrollable_column,
    );

    harness.dispose_owner();
    harness.detach_node();
}

#[test]
fn scroll_node_measure_does_not_observe_scroll_position_in_snapshot() {
    let state_slot: Rc<Cell<Option<ScrollState>>> = Rc::new(Cell::new(None));
    let state_cap = Rc::clone(&state_slot);

    let mut composition = run_test_composition(move || {
        let state = remember(|| ScrollState::new(0.0)).with(|state| *state);
        state_cap.set(Some(state));
        scrollable_column(state);
    });

    layout_and_semantics(&mut composition);
    let root = composition.root().expect("composed root");
    let state = state_slot.get().expect("scroll state");

    let measure_invalidations = Rc::new(Cell::new(0usize));
    let semantics_invalidations = Rc::new(Cell::new(0usize));
    let observer = SnapshotStateObserver::new(|callback| callback());
    observer.start();

    let measure_flag = Rc::clone(&measure_invalidations);
    let resized = Size {
        width: VIEWPORT.width,
        height: VIEWPORT.height - 20.0,
    };
    observer.observe_reads(
        1usize,
        move |_| measure_flag.set(measure_flag.get() + 1),
        || {
            composition.with_runtime_applier(|applier| {
                measure_layout_with_options(
                    applier,
                    root,
                    resized,
                    MeasureLayoutOptions {
                        collect_semantics: false,
                        build_layout_tree: false,
                    },
                )
                .expect("layout succeeds");
            });
        },
    );

    let semantics_flag = Rc::clone(&semantics_invalidations);
    observer.observe_reads(
        2usize,
        move |_| semantics_flag.set(semantics_flag.get() + 1),
        || {
            composition.with_runtime_applier(|applier| {
                build_semantics_tree_from_applier(applier, root).expect("semantics tree builds");
            });
        },
    );

    state.scroll_to(40.0);
    observer.stop();

    assert_eq!(
        measure_invalidations.get(),
        0,
        "ScrollNode::measure must not record a snapshot read of ScrollState::value"
    );
    assert!(
        semantics_invalidations.get() > 0,
        "scroll semantics must continue to observe ScrollState::value"
    );
}

#[test]
fn scroll_state_animation_steps_after_owner_disposes() {
    run_animation_past_owner_disposal(
        || {
            let state = remember(|| ScrollState::new(0.0)).with(|state| *state);
            scrollable_column(state);
            state
        },
        |state| state.animate_scroll_to(120.0),
    );
}

#[test]
fn scroll_state_is_scroll_in_progress_returns_false_after_owner_disposes() {
    let show_slot: Rc<Cell<Option<MutableState<bool>>>> = Rc::new(Cell::new(None));
    let state_slot: Rc<Cell<Option<ScrollState>>> = Rc::new(Cell::new(None));

    let show_cap = Rc::clone(&show_slot);
    let state_cap = Rc::clone(&state_slot);
    let mut composition = run_test_composition(move || {
        let show = rememberMutableStateOf(|| true);
        show_cap.set(Some(show));
        let state_cap = Rc::clone(&state_cap);
        Box(
            Modifier::empty().fill_max_size(),
            BoxSpec::default(),
            move || {
                if show.get() {
                    let state = remember(|| ScrollState::new(0.0)).with(|state| *state);
                    scrollable_column(state);
                    state_cap.set(Some(state));
                }
            },
        );
    });

    layout_and_semantics(&mut composition);
    let state = state_slot.get().expect("scroll state");
    let show = show_slot.get().expect("show switch");

    composition.with_app_context(|| {
        state.animate_scroll_to(120.0);
    });
    assert!(
        state.is_scroll_in_progress(),
        "scroll should report in progress while animation runs"
    );

    show.set(false);
    settle_composition(&mut composition);

    assert!(
        !state.is_scroll_in_progress(),
        "disposed ScrollState must report is_scroll_in_progress() == false"
    );
}

#[test]
fn interaction_node_detaches_after_interaction_source_owner_disposes() {
    let mut harness = SplitOwnerHarness::new(
        3001,
        3002,
        rememberMutableInteractionSource,
        |source: MutableInteractionSource| {
            Box(
                Modifier::empty()
                    .size_points(100.0, 100.0)
                    .hoverable(source, true)
                    .press_interaction_source(source),
                BoxSpec::default(),
                || {},
            );
        },
    );

    // Activate both hover and press so InteractionNode::on_detach must end active interactions.
    let handlers = pointer_handlers(&mut harness.composition);
    assert!(
        !handlers.is_empty(),
        "expected interaction pointer handlers"
    );
    let buttons = PointerButtons::new().with(PointerButton::Primary);
    for handler in &handlers {
        handler(PointerEvent::new(
            PointerEventKind::Enter,
            Point::new(10.0, 10.0),
            Point::new(10.0, 10.0),
        ));
        handler(
            PointerEvent::new(
                PointerEventKind::Down,
                Point::new(10.0, 10.0),
                Point::new(10.0, 10.0),
            )
            .with_buttons(buttons),
        );
    }

    harness.dispose_owner();
    harness.detach_node();
}

#[test]
fn wear_scaling_list_animation_steps_after_state_owner_disposes() {
    run_animation_past_owner_disposal(
        || {
            let state = rememberWearScalingListState(CentreAnchor::default());
            WearScalingLazyColumn(
                Modifier::empty().fill_max_size(),
                state,
                WearScalingLazyColumnSpec::default(),
                |scope| {
                    scope.items(20, |_| {
                        Spacer(Modifier::empty().fill_max_width().height(40.0));
                    });
                },
            );
            state
        },
        |state: WearScalingListState| state.animate_scroll_to_item(12, 0.0),
    );
}

#[test]
fn draggable_cancels_active_drag_after_state_owner_disposes() {
    let mut harness = SplitOwnerHarness::new(
        4001,
        4002,
        || rememberDraggableState(|_| {}),
        |drag: DraggableState| {
            Box(
                Modifier::empty()
                    .size_points(120.0, 120.0)
                    .draggable(Axis::Vertical, drag),
                BoxSpec::default(),
                || {},
            );
        },
    );

    // Begin dragging the draggable node before disposing the state owner.
    let handlers = pointer_handlers(&mut harness.composition);
    let buttons = PointerButtons::new().with(PointerButton::Primary);
    for handler in &handlers {
        handler(
            PointerEvent::new(
                PointerEventKind::Down,
                Point::new(20.0, 20.0),
                Point::new(20.0, 20.0),
            )
            .with_buttons(buttons),
        );
        handler(
            PointerEvent::new(
                PointerEventKind::Move,
                Point::new(20.0, 60.0),
                Point::new(20.0, 60.0),
            )
            .with_buttons(buttons),
        );
    }

    harness.dispose_owner();

    // Cancel the active drag after its owner disposed, then unmount the node.
    for handler in &handlers {
        handler(PointerEvent::new(
            PointerEventKind::Cancel,
            Point::new(20.0, 60.0),
            Point::new(20.0, 60.0),
        ));
    }
    harness.detach_node();
}
