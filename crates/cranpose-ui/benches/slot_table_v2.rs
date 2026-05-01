#![allow(non_snake_case)]

use cranpose_core::{
    location_key, Composition, ContentTypeReusePolicy, Key, MemoryApplier, MutableState,
    RecomposeOptions, RecomposeScope, SlotId, State, SubcomposeState,
};
use cranpose_foundation::lazy::{
    remember_lazy_list_state_with_position, LazyListScope, LazyListState,
};
use cranpose_macros::composable;
use cranpose_ui::{
    measure_layout, Column, ColumnSpec, LazyColumn, LazyColumnSpec, LinearArrangement, Modifier,
    Size, Text, TextStyle,
};
use criterion::{criterion_group, criterion_main, Bencher, Criterion};
use std::cell::Cell;
use std::hint::black_box;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

const KEYED_REVERSE_SIZES: [usize; 5] = [16, 64, 256, 1_024, 4_096];
const KEYED_ROTATE_ITEMS: usize = 1_024;
const KEYED_SHUFFLE_ITEMS: usize = 1_024;
const KEYED_SHUFFLE_ORDER_COUNT: usize = 64;
const KEYED_REVERSE_BENCH_STEPS: usize = 2;
const KEYED_ROTATE_BENCH_STEPS: usize = 16;
const KEYED_SHUFFLE_BENCH_STEPS: usize = 16;
const CONDITIONAL_TOGGLE_ITEMS: usize = 1_024;
const CONDITIONAL_TOGGLE_BENCH_STEPS: usize = 2;
const TAB_PAYLOAD_GROUP_SIZES: [usize; 3] = [16, 192, 1_024];
const TAB_SWITCH_BENCH_STEPS: usize = 2;
const SUBCOMPOSE_TOTAL_SLOTS: usize = 2_048;
const SUBCOMPOSE_VISIBLE_SLOTS: usize = 48;
const SUBCOMPOSE_SCROLL_STEP: usize = 12;
const SUBCOMPOSE_SCROLL_BENCH_STEPS: usize = 16;
const SUBCOMPOSE_CONTENT_TYPES: u64 = 6;
const LAZY_LIST_TOTAL_ITEMS: usize = 2_048;
const LAZY_LIST_START_INDEX: usize = 768;
const LAZY_LIST_NEAR_JUMP: usize = 64;
const LAZY_LIST_FAR_BACKWARD_INDEX: usize = 128;
const LAZY_LIST_FAR_FORWARD_INDEX: usize = LAZY_LIST_TOTAL_ITEMS - 192;
const LAZY_LIST_VIEWPORT_HEIGHT: f32 = 320.0;
const LAZY_LIST_ROOT_SIZE: Size = Size {
    width: 360.0,
    height: 420.0,
};
const LAZY_LIST_SCROLL_PATTERN: [f32; 6] = [-220.0, -180.0, 96.0, 96.0, 104.0, 104.0];
const LAZY_LIST_SCROLL_BENCH_STEPS: usize = LAZY_LIST_SCROLL_PATTERN.len();
const ANIMATION_FRAME_SECTIONS: usize = 16;
const ANIMATION_FRAME_ITEMS_PER_SECTION: usize = 8;
const ANIMATION_FRAME_BENCH_STEPS: usize = 16;

#[composable]
fn KeyedItem(id: u64) {
    let remembered = cranpose_core::remember(|| id);
    black_box(remembered.with(|value| *value));
}

#[composable]
fn KeyedListContent(order: KeyedOrder) {
    for &id in order.items.iter() {
        cranpose_core::with_key(&id, || KeyedItem(id));
    }
}

#[composable]
fn ConditionalToggleContent(item_count: usize, toggle_index: usize, include_toggled: bool) {
    for id in 0..item_count as u64 {
        if id as usize != toggle_index || include_toggled {
            cranpose_core::with_key(&id, || KeyedItem(id));
        }
    }
}

#[composable]
fn TabPayload(seed: u64, groups: usize) {
    for index in 0..groups {
        cranpose_core::with_key(&(seed, index as u64), || {
            let remembered = cranpose_core::remember(|| seed + index as u64);
            black_box(remembered.with(|value| *value));
        });
    }
}

#[composable]
fn TabSwitchContent(active_tab: usize, first_tab_key: Key, second_tab_key: Key, groups: usize) {
    cranpose_core::withCurrentComposer(|composer| match active_tab {
        0 => composer.cranpose_with_reuse(first_tab_key, RecomposeOptions::default(), |_| {
            TabPayload(10_000, groups)
        }),
        _ => composer.cranpose_with_reuse(second_tab_key, RecomposeOptions::default(), |_| {
            TabPayload(20_000, groups)
        }),
    });
}

#[composable]
fn LazyListItem(index: usize) {
    let remembered = cranpose_core::remember(|| index as u64);
    black_box(remembered.with(|value| *value));
    let height = match index % SUBCOMPOSE_CONTENT_TYPES as usize {
        0 => 44.0,
        1 => 72.0,
        2 => 96.0,
        3 => 56.0,
        4 => 128.0,
        _ => 84.0,
    };

    Column(
        Modifier::empty().fill_max_width().height(height),
        ColumnSpec::default(),
        || {},
    );
}

#[composable]
fn LazyListScrollContent(state_capture: Rc<Cell<Option<LazyListState>>>) {
    let list_state = remember_lazy_list_state_with_position(LAZY_LIST_START_INDEX, 0.0);
    state_capture.set(Some(list_state));

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                format!("First visible {}", list_state.first_visible_item_index()),
                Modifier::empty(),
                TextStyle::default(),
            );
            LazyColumn(
                Modifier::empty()
                    .fill_max_width()
                    .height(LAZY_LIST_VIEWPORT_HEIGHT),
                list_state,
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
                |scope| {
                    scope.items(
                        LAZY_LIST_TOTAL_ITEMS,
                        Some(|index: usize| index as u64),
                        Some(|index: usize| (index % SUBCOMPOSE_CONTENT_TYPES as usize) as u64),
                        LazyListItem,
                    );
                },
            );
        },
    );
}

#[composable]
fn AnimationFrameLeaf(progress: State<f32>, section: usize, item: usize) {
    let remembered = cranpose_core::remember(|| (section as u64) << 32 | item as u64);
    let value = progress.value();
    black_box(remembered.with(|id| *id) as f32 + value);
}

#[composable]
fn AnimationFrameSection(progress: State<f32>, section: usize) {
    for item in 0..ANIMATION_FRAME_ITEMS_PER_SECTION {
        AnimationFrameLeaf(progress, section, item);
    }
}

#[composable]
fn AnimationFrameContent(state_capture: Rc<Cell<Option<MutableState<f32>>>>) {
    let progress = cranpose_core::useState(|| 0.0_f32);
    state_capture.set(Some(progress));
    let progress = progress.as_state();
    for section in 0..ANIMATION_FRAME_SECTIONS {
        AnimationFrameSection(progress, section);
    }
}

#[derive(Clone, Copy)]
enum KeyedOrderScenario {
    Reverse,
    RotateFrontToBack,
    Shuffle { seed: u64 },
}

#[derive(Clone)]
struct KeyedOrder {
    id: usize,
    items: Arc<[u64]>,
}

impl PartialEq for KeyedOrder {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

#[derive(Clone, Copy)]
enum ConditionalTogglePosition {
    Front,
    Middle,
    End,
}

impl ConditionalTogglePosition {
    fn item_index(self, item_count: usize) -> usize {
        match self {
            Self::Front => 0,
            Self::Middle => item_count / 2,
            Self::End => item_count - 1,
        }
    }
}

#[derive(Clone, Copy)]
enum LazyListScrollScenario {
    Steady,
    JumpNear,
    JumpFar,
}

struct KeyedReorderFixture {
    composition: Composition<MemoryApplier>,
    root_key: Key,
    orders: Vec<KeyedOrder>,
    order_index: usize,
}

impl KeyedReorderFixture {
    fn new(item_count: usize, scenario: KeyedOrderScenario) -> Self {
        Self {
            composition: Composition::new(MemoryApplier::new()),
            root_key: location_key(file!(), line!(), column!()),
            orders: build_keyed_orders(item_count, scenario),
            order_index: 0,
        }
    }

    fn render(&mut self) {
        let order = self.orders[self.order_index].clone();
        self.composition
            .render(self.root_key, || KeyedListContent(order.clone()))
            .expect("keyed reorder render");
    }

    fn step(&mut self) {
        self.order_index = (self.order_index + 1) % self.orders.len();
        self.render();
    }
}

struct ConditionalToggleFixture {
    composition: Composition<MemoryApplier>,
    root_key: Key,
    item_count: usize,
    toggle_index: usize,
    include_toggled: bool,
}

impl ConditionalToggleFixture {
    fn new(item_count: usize, position: ConditionalTogglePosition) -> Self {
        Self {
            composition: Composition::new(MemoryApplier::new()),
            root_key: location_key(file!(), line!(), column!()),
            item_count,
            toggle_index: position.item_index(item_count),
            include_toggled: true,
        }
    }

    fn render(&mut self) {
        let item_count = self.item_count;
        let toggle_index = self.toggle_index;
        let include_toggled = self.include_toggled;
        self.composition
            .render(self.root_key, || {
                ConditionalToggleContent(item_count, toggle_index, include_toggled)
            })
            .expect("conditional toggle render");
    }

    fn step(&mut self) {
        self.include_toggled = !self.include_toggled;
        self.render();
    }
}

struct TabSwitchFixture {
    composition: Composition<MemoryApplier>,
    root_key: Key,
    first_tab_key: Key,
    second_tab_key: Key,
    active_tab: usize,
    groups: usize,
}

impl TabSwitchFixture {
    fn new(groups: usize) -> Self {
        Self {
            composition: Composition::new(MemoryApplier::new()),
            root_key: location_key(file!(), line!(), column!()),
            first_tab_key: location_key(file!(), line!(), column!()),
            second_tab_key: location_key(file!(), line!(), column!()),
            active_tab: 0,
            groups,
        }
    }

    fn render(&mut self) {
        let active_tab = self.active_tab;
        let first_tab_key = self.first_tab_key;
        let second_tab_key = self.second_tab_key;
        let groups = self.groups;
        self.composition
            .render(self.root_key, || {
                TabSwitchContent(active_tab, first_tab_key, second_tab_key, groups)
            })
            .expect("tab switch render");
    }

    fn step(&mut self) {
        self.active_tab ^= 1;
        self.render();
    }
}

struct SubcomposeScrollFixture {
    state: SubcomposeState,
    next_node_id: usize,
    offset: usize,
}

struct LazyListScrollFixture {
    composition: Composition<MemoryApplier>,
    root_key: Key,
    state_capture: Rc<Cell<Option<LazyListState>>>,
    scenario: LazyListScrollScenario,
    scroll_pattern_index: usize,
    jump_forward: bool,
}

struct AnimationFrameFixture {
    composition: Composition<MemoryApplier>,
    root_key: Key,
    state_capture: Rc<Cell<Option<MutableState<f32>>>>,
    frame: usize,
}

impl LazyListScrollFixture {
    fn new(scenario: LazyListScrollScenario) -> Self {
        let mut fixture = Self {
            composition: Composition::new(MemoryApplier::new()),
            root_key: location_key(file!(), line!(), column!()),
            state_capture: Rc::new(Cell::new(None)),
            scenario,
            scroll_pattern_index: 0,
            jump_forward: true,
        };
        fixture.render();
        fixture.settle();
        fixture
    }

    fn render(&mut self) {
        let state_capture = Rc::clone(&self.state_capture);
        self.composition
            .render(self.root_key, move || {
                LazyListScrollContent(Rc::clone(&state_capture))
            })
            .expect("lazy list render");
    }

    fn list_state(&self) -> LazyListState {
        self.state_capture
            .get()
            .expect("lazy list benchmark state must be captured")
    }

    fn measure(&mut self) {
        let root = self.composition.root().expect("lazy list benchmark root");
        let handle = self.composition.runtime_handle();
        let measurements = {
            let mut applier = self.composition.applier_mut();
            applier.set_runtime_handle(handle);
            let result =
                measure_layout(&mut applier, root, LAZY_LIST_ROOT_SIZE).expect("lazy list measure");
            applier.clear_runtime_handle();
            result
        };
        black_box(measurements.root_size());
    }

    fn settle(&mut self) {
        self.measure();
        while self
            .composition
            .process_invalid_scopes()
            .expect("lazy list scroll recomposition")
        {
            self.measure();
        }
        self.measure();
    }

    fn step(&mut self) {
        let list_state = self.list_state();
        match self.scenario {
            LazyListScrollScenario::Steady => {
                let delta = LAZY_LIST_SCROLL_PATTERN[self.scroll_pattern_index];
                self.scroll_pattern_index =
                    (self.scroll_pattern_index + 1) % LAZY_LIST_SCROLL_PATTERN.len();
                black_box(list_state.dispatch_scroll_delta(delta));
            }
            LazyListScrollScenario::JumpNear => {
                let target = if self.jump_forward {
                    LAZY_LIST_START_INDEX + LAZY_LIST_NEAR_JUMP
                } else {
                    LAZY_LIST_START_INDEX
                };
                self.jump_forward = !self.jump_forward;
                list_state.scroll_to_item(target, 0.0);
            }
            LazyListScrollScenario::JumpFar => {
                let target = if self.jump_forward {
                    LAZY_LIST_FAR_FORWARD_INDEX
                } else {
                    LAZY_LIST_FAR_BACKWARD_INDEX
                };
                self.jump_forward = !self.jump_forward;
                list_state.scroll_to_item(target, 0.0);
            }
        }
        self.settle();

        let stats = list_state.stats();
        let layout_info = list_state.layout_info();
        let first_visible = layout_info
            .visible_items_info
            .first()
            .map(|item| item.index)
            .unwrap_or_default();
        black_box((
            first_visible,
            stats.items_in_use,
            stats.items_in_pool,
            stats.reuse_count,
        ));
    }
}

impl AnimationFrameFixture {
    fn new() -> Self {
        let mut fixture = Self {
            composition: Composition::new(MemoryApplier::new()),
            root_key: location_key(file!(), line!(), column!()),
            state_capture: Rc::new(Cell::new(None)),
            frame: 0,
        };
        fixture.render();
        fixture.settle();
        fixture
    }

    fn render(&mut self) {
        let state_capture = Rc::clone(&self.state_capture);
        self.composition
            .render(self.root_key, move || {
                AnimationFrameContent(Rc::clone(&state_capture))
            })
            .expect("animation frame render");
    }

    fn progress_state(&self) -> MutableState<f32> {
        self.state_capture
            .get()
            .expect("animation benchmark state must be captured")
    }

    fn settle(&mut self) {
        while self
            .composition
            .process_invalid_scopes()
            .expect("animation frame recomposition")
        {}
    }

    fn step(&mut self) {
        self.frame += 1;
        let progress = (self.frame % 120) as f32 / 120.0;
        self.progress_state().set(progress);
        self.settle();
        let stats = self.composition.debug_slot_table_stats();
        black_box((
            stats.group_count,
            stats.payload_count,
            stats.active_anchor_count,
        ));
    }
}

impl SubcomposeScrollFixture {
    fn new() -> Self {
        Self {
            state: SubcomposeState::new(Box::new(ContentTypeReusePolicy::new())),
            next_node_id: 1,
            offset: 0,
        }
    }

    fn next_node_for_slot(&mut self, slot_id: SlotId) -> usize {
        self.state
            .take_node_from_reusables(slot_id)
            .unwrap_or_else(|| {
                let node_id = self.next_node_id;
                self.next_node_id += 1;
                node_id
            })
    }

    fn step(&mut self) {
        const NO_SCOPES: [RecomposeScope; 0] = [];

        self.state.begin_pass();
        for visible_index in 0..SUBCOMPOSE_VISIBLE_SLOTS {
            let item_index = (self.offset + visible_index) % SUBCOMPOSE_TOTAL_SLOTS;
            let slot_id = SlotId::new(item_index as u64);
            self.state
                .register_content_type(slot_id, (item_index as u64) % SUBCOMPOSE_CONTENT_TYPES);
            let _ = self.state.get_or_create_slots(slot_id);
            let node_id = self.next_node_for_slot(slot_id);
            self.state.register_active(slot_id, &[node_id], &NO_SCOPES);
            black_box(node_id);
        }

        let disposed = self.state.finish_pass();
        black_box(disposed.len());

        self.offset = (self.offset + SUBCOMPOSE_SCROLL_STEP) % SUBCOMPOSE_TOTAL_SLOTS;
    }
}

fn next_shuffle_value(state: &mut u64) -> u64 {
    let mut value = *state;
    value ^= value << 13;
    value ^= value >> 7;
    value ^= value << 17;
    *state = value;
    value
}

fn build_keyed_orders(item_count: usize, scenario: KeyedOrderScenario) -> Vec<KeyedOrder> {
    let mut base = (0..item_count as u64).collect::<Vec<_>>();
    match scenario {
        KeyedOrderScenario::Reverse => {
            let mut reversed = base.clone();
            reversed.reverse();
            vec![keyed_order(0, base), keyed_order(1, reversed)]
        }
        KeyedOrderScenario::RotateFrontToBack => {
            let mut orders = Vec::with_capacity(item_count);
            for id in 0..item_count {
                orders.push(keyed_order(id, base.clone()));
                base.rotate_left(1);
            }
            orders
        }
        KeyedOrderScenario::Shuffle { seed } => {
            let mut orders = Vec::with_capacity(KEYED_SHUFFLE_ORDER_COUNT);
            let mut state = seed;
            for id in 0..KEYED_SHUFFLE_ORDER_COUNT {
                orders.push(keyed_order(id, base.clone()));
                shuffle_order(&mut base, &mut state);
            }
            orders
        }
    }
}

fn keyed_order(id: usize, items: Vec<u64>) -> KeyedOrder {
    KeyedOrder {
        id,
        items: Arc::from(items),
    }
}

fn shuffle_order(order: &mut [u64], state: &mut u64) {
    for index in (1..order.len()).rev() {
        let selected = (next_shuffle_value(state) as usize) % (index + 1);
        order.swap(index, selected);
    }
}

fn bench_stateful_steps<Step>(b: &mut Bencher<'_>, steps_per_iteration: usize, mut step: Step)
where
    Step: FnMut(),
{
    assert!(steps_per_iteration > 0);
    b.iter(|| {
        for _ in 0..steps_per_iteration {
            step();
        }
    });
}

fn bench_keyed_reorder_matrix(c: &mut Criterion) {
    for item_count in KEYED_REVERSE_SIZES {
        let mut fixture = KeyedReorderFixture::new(item_count, KeyedOrderScenario::Reverse);
        fixture.render();
        let name = format!("slot_table_v2_keyed_reverse_{item_count}");
        c.bench_function(&name, |b| {
            bench_stateful_steps(b, KEYED_REVERSE_BENCH_STEPS, || fixture.step());
        });
    }

    let mut fixture =
        KeyedReorderFixture::new(KEYED_ROTATE_ITEMS, KeyedOrderScenario::RotateFrontToBack);
    fixture.render();
    c.bench_function("slot_table_v2_keyed_rotate_front_to_back_1024", |b| {
        bench_stateful_steps(b, KEYED_ROTATE_BENCH_STEPS, || fixture.step());
    });

    for seed in [1, 2] {
        let mut fixture =
            KeyedReorderFixture::new(KEYED_SHUFFLE_ITEMS, KeyedOrderScenario::Shuffle { seed });
        fixture.render();
        let name = format!("slot_table_v2_keyed_random_shuffle_1024_seed_{seed}");
        c.bench_function(&name, |b| {
            bench_stateful_steps(b, KEYED_SHUFFLE_BENCH_STEPS, || fixture.step());
        });
    }
}

fn bench_conditional_toggle_matrix(c: &mut Criterion) {
    for (name, position) in [
        ("front", ConditionalTogglePosition::Front),
        ("middle", ConditionalTogglePosition::Middle),
        ("end", ConditionalTogglePosition::End),
    ] {
        let mut fixture = ConditionalToggleFixture::new(CONDITIONAL_TOGGLE_ITEMS, position);
        fixture.render();
        let benchmark_name = format!("slot_table_v2_conditional_toggle_{name}_1024");
        c.bench_function(&benchmark_name, |b| {
            bench_stateful_steps(b, CONDITIONAL_TOGGLE_BENCH_STEPS, || fixture.step());
        });
    }
}

fn bench_tab_switching_matrix(c: &mut Criterion) {
    for groups in TAB_PAYLOAD_GROUP_SIZES {
        let mut fixture = TabSwitchFixture::new(groups);
        fixture.render();
        let name = format!("slot_table_v2_tab_switch_{groups}_payload_groups");
        c.bench_function(&name, |b| {
            bench_stateful_steps(b, TAB_SWITCH_BENCH_STEPS, || fixture.step());
        });
    }
}

fn bench_subcompose_scrolling(c: &mut Criterion) {
    let mut fixture = SubcomposeScrollFixture::new();
    fixture.step();

    c.bench_function("slot_table_v2_subcompose_scrolling", |b| {
        bench_stateful_steps(b, SUBCOMPOSE_SCROLL_BENCH_STEPS, || fixture.step());
    });
}

fn bench_lazy_list_matrix(c: &mut Criterion) {
    for (name, scenario) in [
        (
            "slot_table_v2_lazy_scroll_steady",
            LazyListScrollScenario::Steady,
        ),
        (
            "slot_table_v2_lazy_scroll_jump_near",
            LazyListScrollScenario::JumpNear,
        ),
        (
            "slot_table_v2_lazy_scroll_jump_far",
            LazyListScrollScenario::JumpFar,
        ),
    ] {
        let mut fixture = LazyListScrollFixture::new(scenario);
        c.bench_function(name, |b| {
            bench_stateful_steps(b, LAZY_LIST_SCROLL_BENCH_STEPS, || fixture.step());
        });
    }
}

fn bench_animation_frame_recomposition(c: &mut Criterion) {
    let mut fixture = AnimationFrameFixture::new();
    c.bench_function("slot_table_v2_animation_frame_recompose_128", |b| {
        bench_stateful_steps(b, ANIMATION_FRAME_BENCH_STEPS, || fixture.step());
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(5))
        .sample_size(30);
    targets =
        bench_keyed_reorder_matrix,
        bench_conditional_toggle_matrix,
        bench_tab_switching_matrix,
        bench_subcompose_scrolling,
        bench_lazy_list_matrix,
        bench_animation_frame_recomposition
);
criterion_main!(benches);
