use std::{
    cell::{Cell, RefCell},
    hash::{Hash, Hasher},
    rc::Rc,
    time::{Duration, Instant},
};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_foundation::{
    DelegatableNode, DrawModifierNode, ModifierNode, ModifierNodeElement, NodeCapabilities,
    NodeDrawClosure, NodeState,
    lazy::{LazyListScope, LazyListState, rememberLazyListState},
};
use cranpose_macros::composable;
use cranpose_render_common::{RenderScene, Renderer, graph_scene::Scene};
use cranpose_ui::{Box, BoxSpec, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Modifier};
use cranpose_ui_graphics::Size;

const SLICE_FACTORY_DELAY: Duration = Duration::from_millis(160);

#[derive(Default)]
struct EmptyRenderer(Scene);

impl Renderer for EmptyRenderer {
    type Scene = Scene;
    type Error = std::convert::Infallible;

    fn scene(&self) -> &Self::Scene {
        &self.0
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.0
    }

    fn rebuild_scene(
        &mut self,
        _layout_tree: &cranpose_ui::LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.0.clear();
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        _applier: &mut cranpose_core::MemoryApplier,
        _root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.0.clear();
        Ok(())
    }
}

struct SliceFactoryNode {
    state: NodeState,
    calls: Rc<Cell<usize>>,
}

impl DelegatableNode for SliceFactoryNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for SliceFactoryNode {
    cranpose_foundation::impl_modifier_node!(draw);
}

impl DrawModifierNode for SliceFactoryNode {
    fn create_draw_closure(&self) -> Option<NodeDrawClosure> {
        self.calls.set(self.calls.get() + 1);
        std::thread::sleep(SLICE_FACTORY_DELAY);
        Some(Rc::new(|_| {}))
    }
}

struct SliceFactoryElement(Rc<Cell<usize>>);

impl std::fmt::Debug for SliceFactoryElement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SliceFactoryElement")
    }
}

impl PartialEq for SliceFactoryElement {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SliceFactoryElement {}

impl Hash for SliceFactoryElement {
    fn hash<H: Hasher>(&self, state: &mut H) {
        Rc::as_ptr(&self.0).hash(state);
    }
}

impl ModifierNodeElement for SliceFactoryElement {
    type Node = SliceFactoryNode;

    fn create(&self) -> Self::Node {
        SliceFactoryNode {
            state: NodeState::new(),
            calls: Rc::clone(&self.0),
        }
    }

    fn update(&self, node: &mut Self::Node) {
        node.calls = Rc::clone(&self.0);
    }

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::DRAW
    }
}

#[composable]
fn PrefetchScreen(
    state_capture: Rc<RefCell<Option<LazyListState>>>,
    slice_factory_calls: Rc<Cell<usize>>,
) {
    let state = rememberLazyListState();
    *state_capture.borrow_mut() = Some(state);
    PrefetchList(state, 100, 120.0, slice_factory_calls);
}

#[composable]
fn TwoListPrefetchScreen(
    state_capture: Rc<RefCell<Option<[LazyListState; 2]>>>,
    slice_factory_calls: Rc<Cell<usize>>,
) {
    let long_list_state = rememberLazyListState();
    let short_list_state = rememberLazyListState();
    *state_capture.borrow_mut() = Some([long_list_state, short_list_state]);
    Column(
        Modifier::empty().size_points(320.0, 120.0),
        ColumnSpec::new(),
        move || {
            PrefetchList(long_list_state, 100, 60.0, Rc::clone(&slice_factory_calls));
            PrefetchList(short_list_state, 8, 60.0, Rc::clone(&slice_factory_calls));
        },
    );
}

#[composable]
fn PrefetchList(
    state: LazyListState,
    item_count: usize,
    height: f32,
    slice_factory_calls: Rc<Cell<usize>>,
) {
    let mut spec = LazyColumnSpec::new();
    spec.beyond_bounds_item_count = 0;
    LazyColumn(
        Modifier::empty().size_points(320.0, height),
        state,
        spec,
        move |scope| {
            scope.items(item_count, {
                let slice_factory_calls = Rc::clone(&slice_factory_calls);
                move |_| {
                    let modifier = Modifier::with_element(SliceFactoryElement(Rc::clone(
                        &slice_factory_calls,
                    )))
                    .size_points(320.0, 40.0);
                    Box(modifier, BoxSpec::default(), || {});
                }
            });
        },
    );
}

fn drive_until_prefetch_is_pending(
    shell: &mut AppShell<EmptyRenderer>,
    states: &[LazyListState],
    app_context: &Rc<cranpose_ui::AppContext>,
) -> bool {
    for _ in 0..12 {
        for state in states {
            let _ = state.dispatch_scroll_delta(-120.0);
        }
        shell.update();
        if app_context.enter(cranpose_ui::has_lazy_prefetch_requests) {
            return true;
        }
    }
    false
}

fn shell_with(content: impl FnMut() + 'static) -> AppShell<EmptyRenderer> {
    let mut shell = AppShell::new_with_size(
        EmptyRenderer::default(),
        location_key(file!(), line!(), column!()),
        content,
        (320, 120),
        (320.0, 120.0),
    );
    shell.update();
    shell
}

fn run_one_idle_pass(shell: &mut AppShell<EmptyRenderer>, deadline: Option<Instant>) -> Duration {
    let started = Instant::now();
    let mut frame_checks = 0;
    assert!(shell.run_idle_prefetch(deadline, |_| {
        frame_checks += 1;
        frame_checks > 1
    }));
    started.elapsed()
}

#[test]
fn idle_prefetch_budget_accounts_for_slice_warming_before_starting_another_pass() {
    let state_capture = Rc::new(RefCell::new(None));
    let slice_factory_calls = Rc::new(Cell::new(0));
    let mut shell = shell_with({
        let state_capture = Rc::clone(&state_capture);
        let slice_factory_calls = Rc::clone(&slice_factory_calls);
        move || {
            PrefetchScreen(Rc::clone(&state_capture), Rc::clone(&slice_factory_calls));
        }
    });
    let state = (*state_capture.borrow()).expect("the list state is composed");

    let app_context = Rc::clone(shell.app_context());
    assert!(
        drive_until_prefetch_is_pending(&mut shell, &[state], &app_context),
        "scrolling leaves an item for idle prefetch"
    );

    let first_pass_cost = run_one_idle_pass(&mut shell, None);
    assert_eq!(slice_factory_calls.get(), 1);
    assert!(
        app_context.enter(cranpose_ui::has_lazy_prefetch_requests),
        "the list still has another deferred item"
    );

    let deadline = Instant::now() + first_pass_cost * 3 / 4;
    assert!(
        !shell.run_idle_prefetch(Some(deadline), |_| false),
        "a later pass must be rejected when its known full cost exceeds the remaining wait"
    );
    assert_eq!(
        slice_factory_calls.get(),
        1,
        "the rejected pass does not warm another row"
    );
}

#[test]
fn idle_prefetch_cost_scales_with_lists_pending_after_a_multi_list_pass() {
    let state_capture = Rc::new(RefCell::new(None));
    let slice_factory_calls = Rc::new(Cell::new(0));
    let mut shell = shell_with({
        let state_capture = Rc::clone(&state_capture);
        let slice_factory_calls = Rc::clone(&slice_factory_calls);
        move || {
            TwoListPrefetchScreen(Rc::clone(&state_capture), Rc::clone(&slice_factory_calls));
        }
    });
    let states = (*state_capture.borrow()).expect("both list states are composed");
    let app_context = Rc::clone(shell.app_context());
    assert!(drive_until_prefetch_is_pending(
        &mut shell,
        &states,
        &app_context
    ));

    let first_pass_cost = run_one_idle_pass(&mut shell, None);
    assert_eq!(slice_factory_calls.get(), 2);
    assert!(
        app_context.enter(cranpose_ui::has_lazy_prefetch_requests),
        "the long list still has a deferred item"
    );

    let deadline = Instant::now() + first_pass_cost * 3 / 4;
    assert!(
        !shell.run_idle_prefetch(Some(deadline), |_| false),
        "the prior full pass remains the estimate until a smaller pass is measured"
    );
    assert_eq!(slice_factory_calls.get(), 2);

    let single_list_pass_cost = run_one_idle_pass(&mut shell, None);
    assert_eq!(slice_factory_calls.get(), 3);

    let deadline = Instant::now() + first_pass_cost * 3 / 4;
    assert!(
        single_list_pass_cost < deadline.saturating_duration_since(Instant::now()),
        "the one-list pass is materially cheaper than the two-list pass"
    );
    run_one_idle_pass(&mut shell, Some(deadline));
    assert_eq!(slice_factory_calls.get(), 4);
}
