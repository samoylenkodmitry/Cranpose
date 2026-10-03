use std::{alloc::System, cell::RefCell, rc::Rc, time::Instant};

use cranpose_ui::{
    Box, BoxSpec, LazyColumn, LazyColumnSpec, LazyItems, LazyListScope, LazyListState, Modifier,
    Size, composable, measure_layout, rememberLazyListState, run_test_composition,
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const ITEM_COUNT: usize = 512;
const ROOT_SIZE: Size = Size::new(360.0, 360.0);
const ROW_HEIGHT: f32 = 36.0;
const WARMUP_FRAMES: usize = 32;
const MEASURED_FRAMES: usize = 512;
const STABLE_DELTA: f32 = 0.125;

#[composable]
fn LazyScrollContent(state_capture: Rc<RefCell<Option<LazyListState>>>, roots_per_item: usize) {
    let state = rememberLazyListState();
    *state_capture.borrow_mut() = Some(state);
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

struct Fixture {
    composition: cranpose_ui::TestComposition,
    state: LazyListState,
}

impl Fixture {
    fn new(roots_per_item: usize) -> Self {
        let state_capture = Rc::new(RefCell::new(None));
        let capture = Rc::clone(&state_capture);
        let composition = run_test_composition(move || {
            LazyScrollContent(Rc::clone(&capture), roots_per_item);
        });
        let state = state_capture
            .borrow()
            .as_ref()
            .copied()
            .expect("lazy list state was captured");
        let mut fixture = Self { composition, state };
        fixture.measure();
        fixture
    }

    fn frame(&mut self, scroll_delta: f32) {
        self.state.dispatch_scroll_delta(scroll_delta);
        while self
            .composition
            .process_invalid_scopes()
            .expect("lazy list recomposition")
        {}
        self.measure();
    }

    fn measure(&mut self) {
        let root = self.composition.root().expect("lazy list root");
        let runtime = self.composition.runtime_handle();
        let mut applier = self.composition.applier_mut();
        applier.set_runtime_handle(runtime);
        measure_layout(&mut applier, root, ROOT_SIZE).expect("lazy list layout");
        applier.clear_runtime_handle();
    }
}

fn frame_delta(phase: &str, frame: usize) -> f32 {
    let magnitude = delta_magnitude(phase);
    if frame % 2 == 0 {
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

fn run_case(roots_per_item: usize, phase: &str) {
    let mut fixture = Fixture::new(roots_per_item);
    for frame in 0..WARMUP_FRAMES {
        fixture.frame(frame_delta(phase, frame));
    }

    let region = Region::new(GLOBAL);
    let start = Instant::now();
    for frame in 0..MEASURED_FRAMES {
        fixture.frame(frame_delta(phase, frame));
    }
    let elapsed = start.elapsed().as_nanos();
    let stats = region.change();
    println!(
        "{{\"benchmark\":\"lazy_scroll\",\"roots_per_item\":{roots_per_item},\"phase\":\"{phase}\",\"warmup_frames\":{WARMUP_FRAMES},\"frames\":{MEASURED_FRAMES},\"alternating_delta_dp\":{},\"allocations\":{},\"reallocations\":{},\"bytes_allocated\":{},\"bytes_deallocated\":{},\"elapsed_ns\":{elapsed},\"ns_per_frame\":{}}}",
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
        run_case(roots_per_item, "stable_retained");
        run_case(roots_per_item, "one_row_entering");
    }
}
