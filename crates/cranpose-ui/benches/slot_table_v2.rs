use cranpose_core::{
    location_key, Composition, ContentTypeReusePolicy, Key, MemoryApplier, RecomposeOptions,
    RecomposeScope, SlotId, SubcomposeState,
};
use cranpose_foundation::lazy::{
    remember_lazy_list_state_with_position, LazyListScope, LazyListState,
};
use cranpose_macros::composable;
use cranpose_ui::{
    measure_layout, Column, ColumnSpec, LazyColumn, LazyColumnSpec, LinearArrangement, Modifier,
    Size, Text, TextStyle,
};
use criterion::{criterion_group, criterion_main, Criterion};
use std::cell::Cell;
use std::hint::black_box;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

const KEYED_REORDER_ITEMS: usize = 256;
const TAB_PAYLOAD_GROUPS: usize = 192;
const SUBCOMPOSE_TOTAL_SLOTS: usize = 2_048;
const SUBCOMPOSE_VISIBLE_SLOTS: usize = 48;
const SUBCOMPOSE_SCROLL_STEP: usize = 12;
const SUBCOMPOSE_CONTENT_TYPES: u64 = 6;
const LAZY_LIST_TOTAL_ITEMS: usize = 2_048;
const LAZY_LIST_START_INDEX: usize = 768;
const LAZY_LIST_VIEWPORT_HEIGHT: f32 = 320.0;
const LAZY_LIST_ROOT_SIZE: Size = Size {
    width: 360.0,
    height: 420.0,
};
const LAZY_LIST_SCROLL_PATTERN: [f32; 6] = [-220.0, -180.0, 96.0, 96.0, 104.0, 104.0];

#[composable]
fn keyed_item(id: u64) {
    let remembered = cranpose_core::remember(|| id);
    black_box(remembered.with(|value| *value));
}

#[composable]
fn keyed_list_content(items: Arc<[u64]>, reversed: bool) {
    if reversed {
        for &id in items.iter().rev() {
            cranpose_core::with_key(&id, || keyed_item(id));
        }
    } else {
        for &id in items.iter() {
            cranpose_core::with_key(&id, || keyed_item(id));
        }
    }
}

#[composable]
fn tab_payload(seed: u64, groups: usize) {
    for index in 0..groups {
        cranpose_core::with_key(&(seed, index as u64), || {
            let remembered = cranpose_core::remember(|| seed + index as u64);
            black_box(remembered.with(|value| *value));
        });
    }
}

#[composable]
fn tab_switch_content(active_tab: usize, first_tab_key: Key, second_tab_key: Key, groups: usize) {
    cranpose_core::withCurrentComposer(|composer| match active_tab {
        0 => composer.cranpose_with_reuse(first_tab_key, RecomposeOptions::default(), |_| {
            tab_payload(10_000, groups)
        }),
        _ => composer.cranpose_with_reuse(second_tab_key, RecomposeOptions::default(), |_| {
            tab_payload(20_000, groups)
        }),
    });
}

#[composable]
fn lazy_list_item(index: usize) {
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
fn lazy_list_scroll_content(state_capture: Rc<Cell<Option<LazyListState>>>) {
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
                        lazy_list_item,
                    );
                },
            );
        },
    );
}

struct KeyedReorderFixture {
    composition: Composition<MemoryApplier>,
    root_key: Key,
    items: Arc<[u64]>,
    reversed: bool,
}

impl KeyedReorderFixture {
    fn new(item_count: usize) -> Self {
        Self {
            composition: Composition::new(MemoryApplier::new()),
            root_key: location_key(file!(), line!(), column!()),
            items: Arc::from((0..item_count as u64).collect::<Vec<_>>()),
            reversed: false,
        }
    }

    fn render(&mut self) {
        let items = Arc::clone(&self.items);
        let reversed = self.reversed;
        self.composition
            .render(self.root_key, || {
                keyed_list_content(Arc::clone(&items), reversed)
            })
            .expect("keyed reorder render");
    }

    fn step(&mut self) {
        self.reversed = !self.reversed;
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
                tab_switch_content(active_tab, first_tab_key, second_tab_key, groups)
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
    scroll_pattern_index: usize,
}

impl LazyListScrollFixture {
    fn new() -> Self {
        let mut fixture = Self {
            composition: Composition::new(MemoryApplier::new()),
            root_key: location_key(file!(), line!(), column!()),
            state_capture: Rc::new(Cell::new(None)),
            scroll_pattern_index: 0,
        };
        fixture.render();
        fixture.settle();
        fixture
    }

    fn render(&mut self) {
        let state_capture = Rc::clone(&self.state_capture);
        self.composition
            .render(self.root_key, move || {
                lazy_list_scroll_content(Rc::clone(&state_capture))
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
        let delta = LAZY_LIST_SCROLL_PATTERN[self.scroll_pattern_index];
        self.scroll_pattern_index =
            (self.scroll_pattern_index + 1) % LAZY_LIST_SCROLL_PATTERN.len();

        let list_state = self.list_state();
        black_box(list_state.dispatch_scroll_delta(delta));
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

fn bench_keyed_list_reorder(c: &mut Criterion) {
    let mut fixture = KeyedReorderFixture::new(KEYED_REORDER_ITEMS);
    fixture.render();

    c.bench_function("slot_table_v2_keyed_list_reorder", |b| {
        b.iter(|| fixture.step());
    });
}

fn bench_tab_switching(c: &mut Criterion) {
    let mut fixture = TabSwitchFixture::new(TAB_PAYLOAD_GROUPS);
    fixture.render();

    c.bench_function("slot_table_v2_tab_switching", |b| {
        b.iter(|| fixture.step());
    });
}

fn bench_subcompose_scrolling(c: &mut Criterion) {
    let mut fixture = SubcomposeScrollFixture::new();
    fixture.step();

    c.bench_function("slot_table_v2_subcompose_scrolling", |b| {
        b.iter(|| fixture.step());
    });
}

fn bench_lazy_list_scroll_reuse(c: &mut Criterion) {
    let mut fixture = LazyListScrollFixture::new();

    c.bench_function("slot_table_v2_lazy_list_scroll_reuse", |b| {
        b.iter(|| fixture.step());
    });
}

criterion_group!(
    name = benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(5))
        .sample_size(30);
    targets =
        bench_keyed_list_reorder,
        bench_tab_switching,
        bench_subcompose_scrolling,
        bench_lazy_list_scroll_reuse
);
criterion_main!(benches);
