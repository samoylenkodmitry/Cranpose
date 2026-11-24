//! Amplifies layout cost for `SubcomposeLayoutNode` modifier chains, wrapped
//! `LazyRow` lists, and dense `LayoutNode` modifier chains under per-frame
//! relayout. Supports standalone JSON output (`CASE=... FRAMES=...`) and
//! `--steady-ledger` for `scripts/perf/steady_ledger.sh`.

#![allow(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "a benchmark prints its measurements and steady-ledger phase markers"
)]

use std::{alloc::System, cell::RefCell, hint::black_box, rc::Rc, time::Instant};

use cranpose_core::{Composition, MemoryApplier, location_key};
use cranpose_ui::{
    AppContext, Box, BoxSpec, BoxWithConstraints, Column, ColumnSpec, LazyItems, LazyListScope,
    LazyListState, LazyRow, LazyRowSpec, Modifier, Size, composable,
    layout::{MeasureLayoutOptions, measure_layout_with_options},
    rememberLazyListState,
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const SUBCOMPOSE_COUNT: usize = 128;
const LAZY_ROW_COUNT: usize = 32;
const LAZY_ITEMS_PER_ROW: usize = 16;
const MODIFIER_NODE_COUNT: usize = 1024;
const DEFAULT_WARMUP: usize = 16;
const DEFAULT_FRAMES: usize = 256;
// Divides the steady ledger's 60-frame window evenly so warmup and steady
// phases end at the same width and scroll phase.
const WIDTH_CYCLE: usize = 12;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    SubcomposeModifiers,
    LazyRowsBounded,
    LayoutModifierChain,
}

impl Case {
    const ALL: [Self; 3] = [
        Self::SubcomposeModifiers,
        Self::LazyRowsBounded,
        Self::LayoutModifierChain,
    ];

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "subcompose_modifiers" => Some(Self::SubcomposeModifiers),
            "lazy_rows_bounded" => Some(Self::LazyRowsBounded),
            "layout_modifier_chain" => Some(Self::LayoutModifierChain),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::SubcomposeModifiers => "subcompose_modifiers",
            Self::LazyRowsBounded => "lazy_rows_bounded",
            Self::LayoutModifierChain => "layout_modifier_chain",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Json,
    SteadyLedger,
}

struct Config {
    case: Option<Case>,
    warmup: usize,
    frames: usize,
    output: OutputFormat,
}

impl Config {
    fn from_args() -> Self {
        let mut case = std::env::var("CASE").ok().and_then(|v| Case::parse(&v));
        let mut warmup = env_usize("WARMUP", DEFAULT_WARMUP);
        let mut frames = env_usize("FRAMES", DEFAULT_FRAMES);
        let mut output = OutputFormat::Json;

        for arg in std::env::args().skip(1) {
            if arg == "--steady-ledger" {
                output = OutputFormat::SteadyLedger;
            } else if let Some(value) = arg.strip_prefix("--case=") {
                case = Some(Case::parse(value).unwrap_or_else(|| {
                    panic!("unknown subcompose_chain case: {value}");
                }));
            } else if let Some(value) = arg.strip_prefix("--warmup=") {
                warmup = value.parse().expect("valid --warmup=<usize>");
            } else if let Some(value) = arg.strip_prefix("--frames=") {
                frames = value.parse().expect("valid --frames=<usize>");
            }
        }
        Self {
            case,
            warmup,
            frames,
            output,
        }
    }
}

#[composable]
fn SubcomposeModifiersScene(count: usize) {
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            for _ in 0..count {
                BoxWithConstraints(
                    Modifier::empty()
                        .fill_max_width()
                        .padding(2.0)
                        .size_in(Size::new(0.0, 12.0), Size::new(f32::INFINITY, 20.0)),
                    move |_scope| {
                        Box(
                            Modifier::empty().fill_max_width().height(16.0),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            }
        },
    );
}

#[composable]
fn LazyRowsBoundedScene(
    rows: usize,
    items: usize,
    states_capture: Rc<RefCell<Vec<LazyListState>>>,
) {
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            for _ in 0..rows {
                let state = rememberLazyListState();
                states_capture.borrow_mut().push(state);
                LazyRow(
                    Modifier::empty().fill_max_width().padding(2.0).height(24.0),
                    state,
                    LazyRowSpec::default(),
                    move |scope| {
                        scope.items(LazyItems::new(items).key(|i| i as u64), move |_| {
                            Box(
                                Modifier::empty().width(48.0).height(20.0),
                                BoxSpec::default(),
                                || {},
                            );
                        });
                    },
                );
            }
        },
    );
}

#[composable]
fn LayoutModifierChainScene(count: usize) {
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            for _ in 0..count {
                Box(
                    Modifier::empty()
                        .fill_max_width()
                        .padding(1.0)
                        .size_in(Size::new(0.0, 2.0), Size::new(f32::INFINITY, 6.0)),
                    BoxSpec::default(),
                    || {},
                );
            }
        },
    );
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn measure_once(composition: &mut Composition<MemoryApplier>, width: f32) {
    let root = composition.root().expect("composition root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    measure_layout_with_options(
        &mut applier,
        root,
        Size::new(width, 2000.0),
        MeasureLayoutOptions {
            collect_semantics: false,
            build_layout_tree: false,
        },
    )
    .expect("layout pass");
    applier.clear_runtime_handle();
}

fn step_frame(
    case: Case,
    composition: &mut Composition<MemoryApplier>,
    states: &[LazyListState],
    frame: usize,
) {
    let width = 360.0 + ((frame % WIDTH_CYCLE) as f32) * 2.0;
    if case == Case::LazyRowsBounded {
        let delta = if frame.is_multiple_of(2) { -0.5 } else { 0.5 };
        for state in states {
            let _ = state.dispatch_scroll_delta(delta);
        }
        while composition
            .process_invalid_scopes()
            .expect("scroll recomposition")
        {}
    }
    measure_once(composition, width);
}

#[inline(never)]
fn steady_frames(
    case: Case,
    composition: &mut Composition<MemoryApplier>,
    states: &[LazyListState],
    start_frame: usize,
    frames: usize,
) {
    for offset in 0..frames {
        step_frame(case, composition, states, start_frame + offset);
    }
}

fn begin_phase(output: OutputFormat, phase: &str) {
    if output == OutputFormat::SteadyLedger {
        eprintln!("steady_frames: phase {phase}");
    }
}

fn run_case(case: Case, config: &Config) {
    let app_context = AppContext::new();
    let states_capture: Rc<RefCell<Vec<LazyListState>>> = Rc::new(RefCell::new(Vec::new()));
    let capture = Rc::clone(&states_capture);

    app_context.enter(|| {
        begin_phase(config.output, "start");
        let start_region = Region::new(GLOBAL);
        let mut composition = Composition::new(MemoryApplier::new());
        composition
            .render(location_key(file!(), line!(), column!()), || match case {
                Case::SubcomposeModifiers => SubcomposeModifiersScene(SUBCOMPOSE_COUNT),
                Case::LazyRowsBounded => {
                    LazyRowsBoundedScene(LAZY_ROW_COUNT, LAZY_ITEMS_PER_ROW, Rc::clone(&capture));
                }
                Case::LayoutModifierChain => LayoutModifierChainScene(MODIFIER_NODE_COUNT),
            })
            .expect("initial composition");
        measure_once(&mut composition, 360.0);
        let started = start_region.change();
        let states = states_capture.borrow().clone();

        begin_phase(config.output, "warmup");
        for frame in 0..config.warmup {
            step_frame(case, &mut composition, &states, frame);
        }

        begin_phase(config.output, "steady");
        let steady_region = Region::new(GLOBAL);
        let started_at = Instant::now();
        steady_frames(case, &mut composition, &states, config.warmup, config.frames);
        let elapsed_ns = started_at.elapsed().as_nanos();
        let steady = steady_region.change();
        begin_phase(config.output, "end");
        black_box(&mut composition);

        match config.output {
            OutputFormat::SteadyLedger => {
                std::mem::forget(composition);
                std::mem::forget(states);
                let held = GLOBAL.stats();
                let live_bytes = held.bytes_allocated as i128 - held.bytes_deallocated as i128
                    + held.bytes_reallocated as i128;
                println!(
                    "steady_frames warmup={} frames={} start_malloc={} start_realloc={} \
                     start_free={} nodes=0 malloc={} realloc={} free={} live_bytes={live_bytes}",
                    config.warmup,
                    config.frames,
                    started.allocations,
                    started.reallocations,
                    started.deallocations,
                    steady.allocations,
                    steady.reallocations,
                    steady.deallocations,
                );
            }
            OutputFormat::Json => {
                let ns_per_frame = if config.frames > 0 {
                    elapsed_ns / config.frames as u128
                } else {
                    0
                };
                println!(
                    "{{\"benchmark\":\"subcompose_chain\",\"case\":\"{}\",\"frames\":{},\"allocations\":{},\"bytes_allocated\":{},\"elapsed_ns\":{},\"ns_per_frame\":{}}}",
                    case.name(),
                    config.frames,
                    steady.allocations,
                    steady.bytes_allocated,
                    elapsed_ns,
                    ns_per_frame,
                );
            }
        }
    });
}

fn main() {
    let config = Config::from_args();
    match config.case {
        Some(case) => run_case(case, &config),
        None => {
            for case in Case::ALL {
                run_case(case, &config);
            }
        }
    }
}
