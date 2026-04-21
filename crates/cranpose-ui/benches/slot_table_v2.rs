use cranpose_core::{
    location_key, Composition, ContentTypeReusePolicy, Key, MemoryApplier, RecomposeOptions,
    RecomposeScope, SlotId, SubcomposeState,
};
use cranpose_macros::composable;
use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;
use std::sync::Arc;

const KEYED_REORDER_ITEMS: usize = 256;
const TAB_PAYLOAD_GROUPS: usize = 192;
const SUBCOMPOSE_TOTAL_SLOTS: usize = 2_048;
const SUBCOMPOSE_VISIBLE_SLOTS: usize = 48;
const SUBCOMPOSE_SCROLL_STEP: usize = 12;
const SUBCOMPOSE_CONTENT_TYPES: u64 = 6;

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

criterion_group!(
    benches,
    bench_keyed_list_reorder,
    bench_tab_switching,
    bench_subcompose_scrolling
);
criterion_main!(benches);
