use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{MutableState, key, remember, rememberMutableStateOf};
use cranpose_foundation::{
    PointerButton, PointerButtons, PointerEvent, PointerEventKind,
    lazy::{LazyListState, rememberLazyListState},
};
use cranpose_ui::{
    Axis, Box, BoxSpec, Column, ColumnSpec, DraggableState, Modifier, MutableInteractionSource,
    Point, ScrollState, Size, Spacer, TestComposition, build_layout_tree_from_applier,
    build_semantics_tree_from_applier, measure_layout, rememberDraggableState,
    rememberMutableInteractionSource,
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
        let _ = build_semantics_tree_from_applier(applier, root);
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

struct SplitOwnerHarness<T> {
    composition: TestComposition,
    keep_owner: MutableState<bool>,
    keep_node: MutableState<bool>,
    shared: Rc<RefCell<Option<T>>>,
}

impl<T: Clone + 'static> SplitOwnerHarness<T> {
    fn new(
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
            shared,
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
        |state| {
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
        },
    );

    harness.dispose_owner();
    harness.detach_node();
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
    let show_slot: Rc<Cell<Option<MutableState<bool>>>> = Rc::new(Cell::new(None));
    let state_slot: Rc<Cell<Option<WearScalingListState>>> = Rc::new(Cell::new(None));

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
                    let state = rememberWearScalingListState(CentreAnchor::default());
                    state_cap.set(Some(state));
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
                }
            },
        );
    });

    layout_and_semantics(&mut composition);
    let state = state_slot.get().expect("scaling list state");
    let show = show_slot.get().expect("show switch");

    let handle = composition.runtime_handle();
    composition.with_app_context(|| {
        state.animate_scroll_to_item(12, 0.0);
    });
    composition.with_app_context(|| {
        handle.drain_frame_callbacks(16_000_000);
    });

    // Unmount the list while the frame animation callback is still registered.
    show.set(false);
    settle_composition(&mut composition);

    composition.with_app_context(|| {
        handle.drain_frame_callbacks(32_000_000);
    });
}

#[test]
fn draggable_and_lazy_list_handle_disposed_state_owner() {
    let mut harness = SplitOwnerHarness::new(
        4001,
        4002,
        || (rememberDraggableState(|_| {}), rememberLazyListState()),
        |(drag, _lazy): (DraggableState, LazyListState)| {
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

    let lazy = harness.shared.borrow().as_ref().expect("lazy state").1;
    harness.dispose_owner();

    assert_eq!(lazy.total_items_count(), 0);

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
