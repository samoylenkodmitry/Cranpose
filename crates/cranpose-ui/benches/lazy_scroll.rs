use std::{alloc::System, cell::RefCell, rc::Rc, time::Instant};

use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, LazyColumn, LazyColumnSpec, LazyItems, LazyListScope,
    LazyListState, Modifier, ScrollState, Size, composable, measure_layout, rememberLazyListState,
    run_test_composition,
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const ITEM_COUNT: usize = 512;
const COLUMN_ROW_COUNT: usize = 32;
const ROOT_SIZE: Size = Size::new(360.0, 360.0);
const ROW_HEIGHT: f32 = 36.0;
const WARMUP_FRAMES: usize = 32;
const MEASURED_FRAMES: usize = 512;
const STABLE_DELTA: f32 = 0.125;

#[derive(Clone, Copy, PartialEq)]
enum ScrollModel {
    Lazy(LazyListState),
    Column(ScrollState),
}

#[composable]
fn LazyScrollContent(state_capture: Rc<RefCell<Option<ScrollModel>>>, roots_per_item: usize) {
    let state = rememberLazyListState();
    *state_capture.borrow_mut() = Some(ScrollModel::Lazy(state));
    LazyColumn(
        Modifier::empty().fill_max_size(),
        state,
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(
                LazyItems::new(ITEM_COUNT).key(|index| index as u64),
                move |_| {
                    let child_height = ROW_HEIGHT / roots_per_item as f32;
                    for _ in 0..roots_per_item {
                        Box(
                            Modifier::empty().fill_max_width().height(child_height),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                },
            );
        },
    );
}

#[composable]
fn ColumnScrollContent(state_capture: Rc<RefCell<Option<ScrollModel>>>) {
    let state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
    *state_capture.borrow_mut() = Some(ScrollModel::Column(state));
    Column(
        Modifier::empty()
            .fill_max_size()
            .vertical_scroll(state, false),
        ColumnSpec::default(),
        move || {
            for _ in 0..COLUMN_ROW_COUNT {
                Box(
                    Modifier::empty().fill_max_width().height(ROW_HEIGHT),
                    BoxSpec::default(),
                    || {},
                );
            }
        },
    );
}

#[derive(Clone, Copy)]
enum FixtureContent {
    Lazy { roots_per_item: usize },
    Column,
}

struct Fixture {
    composition: cranpose_ui::TestComposition,
    scroll: ScrollModel,
}

impl Fixture {
    fn new(content: FixtureContent) -> Self {
        let state_capture = Rc::new(RefCell::new(None));
        let capture = Rc::clone(&state_capture);
        let composition = run_test_composition(move || match content {
            FixtureContent::Lazy { roots_per_item } => {
                LazyScrollContent(Rc::clone(&capture), roots_per_item);
            }
            FixtureContent::Column => ColumnScrollContent(Rc::clone(&capture)),
        });
        let scroll = state_capture
            .borrow()
            .as_ref()
            .copied()
            .expect("scroll state was captured");
        let mut fixture = Self {
            composition,
            scroll,
        };
        fixture.measure();
        fixture
    }

    fn frame(&mut self, scroll_delta: f32) -> f32 {
        let consumed = match self.scroll {
            ScrollModel::Lazy(state) => state.dispatch_scroll_delta(scroll_delta),
            ScrollModel::Column(state) => -state.dispatch_raw_delta(-scroll_delta),
        };
        while self
            .composition
            .process_invalid_scopes()
            .expect("scroll recomposition")
        {}
        self.measure();
        consumed
    }

    fn measure(&mut self) {
        let root = self.composition.root().expect("lazy list root");
        let runtime = self.composition.runtime_handle();
        let mut applier = self.composition.applier_mut();
        applier.set_runtime_handle(runtime);
        measure_layout(&mut applier, root, ROOT_SIZE).expect("scroll layout");
        applier.clear_runtime_handle();
    }
}

fn frame_delta(phase: &str, frame: usize) -> f32 {
    let magnitude = delta_magnitude(phase);
    if frame.is_multiple_of(2) {
        -magnitude
    } else {
        magnitude
    }
}

fn delta_magnitude(phase: &str) -> f32 {
    match phase {
        "stable_retained" => STABLE_DELTA,
        "one_row_entering" => ROW_HEIGHT,
        _ => unreachable!("unknown benchmark phase"),
    }
}

fn run_case(content: FixtureContent, phase: &str) {
    let mut fixture = Fixture::new(content);
    for frame in 0..WARMUP_FRAMES {
        fixture.frame(frame_delta(phase, frame));
    }

    let region = Region::new(GLOBAL);
    let start = Instant::now();
    for frame in 0..MEASURED_FRAMES {
        let requested = frame_delta(phase, frame);
        let consumed = fixture.frame(requested);
        assert_ne!(consumed, 0.0, "scroll delta was not consumed: {requested}");
    }
    let elapsed = start.elapsed().as_nanos();
    let stats = region.change();
    let (benchmark, count_key, count) = match content {
        FixtureContent::Lazy { roots_per_item } => {
            ("lazy_scroll", "roots_per_item", roots_per_item)
        }
        FixtureContent::Column => {
            let ScrollModel::Column(state) = fixture.scroll else {
                unreachable!("column fixture has a scroll state");
            };
            let metrics = state.metrics();
            assert!(metrics.max_offset > 0.0, "column content must scroll");
            let offset = metrics.offset;
            assert!(
                offset.abs() < 0.001,
                "alternating scroll offset drifted: {offset}"
            );
            ("column_scroll", "rows", COLUMN_ROW_COUNT)
        }
    };
    println!(
        "{{\"benchmark\":\"{benchmark}\",\"{count_key}\":{count},\"phase\":\"{phase}\",\"warmup_frames\":{WARMUP_FRAMES},\"frames\":{MEASURED_FRAMES},\"alternating_delta_dp\":{},\"allocations\":{},\"reallocations\":{},\"bytes_allocated\":{},\"bytes_deallocated\":{},\"elapsed_ns\":{elapsed},\"ns_per_frame\":{}}}",
        delta_magnitude(phase),
        stats.allocations,
        stats.reallocations,
        stats.bytes_allocated,
        stats.bytes_deallocated,
        elapsed / MEASURED_FRAMES as u128,
    );
}

fn main() {
    for roots_per_item in [1, 9] {
        let content = FixtureContent::Lazy { roots_per_item };
        run_case(content, "stable_retained");
        run_case(content, "one_row_entering");
    }
    run_case(FixtureContent::Column, "stable_retained");
    run_case(FixtureContent::Column, "one_row_entering");
}
