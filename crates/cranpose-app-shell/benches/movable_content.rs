#![allow(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "a benchmark prints its measurements and steady-ledger phase markers"
)]

use std::{alloc::System, cell::Cell, hint::black_box, rc::Rc, time::Instant};

use cranpose_app_shell::AppShell;
use cranpose_core::{
    MutableState, location_key, rememberMovableContentOf, rememberMutableStateOf, with_key,
};
use cranpose_foundation::lazy::{LazyItems, LazyListScope, LazyListState, rememberLazyListState};
use cranpose_liquid::{LiquidContextMenu, LiquidShape, LiquidTheme, LiquidThemeSpec, SchemeMode};
use cranpose_macros::composable;
use cranpose_ui::{Box, BoxSpec, Color, LazyColumn, LazyColumnSpec, Modifier, Text, TextStyle};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[path = "support/graph_renderer.rs"]
mod graph_renderer;
use graph_renderer::GraphRenderer;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const DEFAULT_WARMUP: usize = 32;
const DEFAULT_FRAMES: usize = 128;
const ITEM_COUNT: usize = 2048;
const ROW_HEIGHT: f32 = 36.0;
const VIEWPORT: (u32, u32) = (360, 360);
const HEAVY_ITEM_COUNT: usize = 16_384;
const HEAVY_ROW_HEIGHT: f32 = 4.0;
const HEAVY_ROWS_PER_FRAME: f32 = 32.0;
const HEAVY_MOVABLES_PER_ROW: usize = 8;
const HEAVY_VIEWPORT: (u32, u32) = (360, 2048);

#[derive(Clone, Copy, PartialEq, Eq)]
enum BenchCase {
    LazyMovableScroll,
    LazyMovableHeavy,
    LazyContextMenuScroll,
    MovableParentFlip,
}

impl BenchCase {
    const ALL: [Self; 4] = [
        Self::LazyMovableScroll,
        Self::LazyMovableHeavy,
        Self::LazyContextMenuScroll,
        Self::MovableParentFlip,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::LazyMovableScroll => "lazy_movable_scroll",
            Self::LazyMovableHeavy => "lazy_movable_heavy",
            Self::LazyContextMenuScroll => "lazy_context_menu_scroll",
            Self::MovableParentFlip => "movable_parent_flip",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "lazy_movable_scroll" => Some(Self::LazyMovableScroll),
            "lazy_movable_heavy" => Some(Self::LazyMovableHeavy),
            "lazy_context_menu_scroll" => Some(Self::LazyContextMenuScroll),
            "movable_parent_flip" => Some(Self::MovableParentFlip),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Json,
    SteadyLedger,
}

struct Config {
    case: Option<BenchCase>,
    warmup: usize,
    frames: usize,
    output: OutputFormat,
}

impl Config {
    fn from_args() -> Self {
        let mut case = None;
        let mut warmup = DEFAULT_WARMUP;
        let mut frames = DEFAULT_FRAMES;
        let mut output = OutputFormat::Json;
        for arg in std::env::args().skip(1) {
            if let Some(value) = arg.strip_prefix("--case=") {
                case = Some(
                    BenchCase::parse(value)
                        .unwrap_or_else(|| panic!("unknown --case value: {value}")),
                );
            } else if let Some(value) = arg.strip_prefix("--warmup=") {
                warmup = value.parse().expect("valid --warmup");
            } else if let Some(value) = arg.strip_prefix("--frames=") {
                frames = value.parse().expect("valid --frames");
            } else if arg == "--steady-ledger" {
                output = OutputFormat::SteadyLedger;
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

enum Driver {
    Lazy {
        state: Rc<Cell<Option<LazyListState>>>,
        scroll_delta: f32,
    },
    Flip(Rc<Cell<Option<MutableState<bool>>>>),
}

impl Driver {
    fn step(&self, shell: &mut AppShell<GraphRenderer>) {
        match self {
            Self::Lazy {
                state: handle,
                scroll_delta,
            } => {
                let state = handle.get().expect("lazy list state captured");
                let consumed = shell
                    .app_context()
                    .enter(|| state.dispatch_scroll_delta(*scroll_delta));
                assert!(consumed.abs() > 0.0, "lazy scroll frame must advance");
                shell.update();
                while shell.run_idle_prefetch(None, |_| false) {}
            }
            Self::Flip(handle) => {
                let side = handle.get().expect("flip state captured");
                shell.app_context().enter(|| side.set(!side.get()));
                shell.update();
            }
        }
    }
}

#[composable]
fn MovableRowBody(index: usize) {
    let content = rememberMovableContentOf(move || {
        Box(
            Modifier::empty()
                .fill_max_width()
                .height(ROW_HEIGHT)
                .background(Color((index % 7) as f32 / 7.0, 0.35, 0.75, 1.0)),
            BoxSpec::default(),
            move || {
                Text(
                    format!("Movable row {index}"),
                    Modifier::empty(),
                    TextStyle::default(),
                );
            },
        );
    });
    Box(
        Modifier::empty().fill_max_width().height(ROW_HEIGHT),
        BoxSpec::default(),
        move || content.show(),
    );
}

#[composable]
fn HeavyMovableRowBody(index: usize) {
    Box(
        Modifier::empty().fill_max_width().height(HEAVY_ROW_HEIGHT),
        BoxSpec::default(),
        move || {
            for slot in 0..HEAVY_MOVABLES_PER_ROW {
                with_key(&slot, || {
                    let content = rememberMovableContentOf(move || {
                        Box(
                            Modifier::empty()
                                .size_points(40.0, HEAVY_ROW_HEIGHT)
                                .background(Color(
                                    ((index + slot) % 7) as f32 / 7.0,
                                    0.35,
                                    0.75,
                                    1.0,
                                )),
                            BoxSpec::default(),
                            || {},
                        );
                    });
                    Box(
                        Modifier::empty().size_points(40.0, HEAVY_ROW_HEIGHT),
                        BoxSpec::default(),
                        move || content.show(),
                    );
                });
            }
        },
    );
}

#[composable]
fn ContextMenuRowBody(index: usize) {
    LiquidContextMenu(
        Modifier::empty().fill_max_width().height(ROW_HEIGHT),
        LiquidShape::RoundedRect(10.0),
        "Actions",
        |_| {},
        move || {
            Text(
                format!("Menu row {index}"),
                Modifier::empty().fill_max_width().height(ROW_HEIGHT),
                TextStyle::default(),
            );
        },
    );
}

#[composable]
fn LazyMovableList(handle: Rc<Cell<Option<LazyListState>>>, case: BenchCase) {
    let list = rememberLazyListState();
    handle.set(Some(list));
    let item_count = match case {
        BenchCase::LazyMovableHeavy => HEAVY_ITEM_COUNT,
        BenchCase::LazyMovableScroll | BenchCase::LazyContextMenuScroll => ITEM_COUNT,
        BenchCase::MovableParentFlip => unreachable!("flip uses its own root"),
    };
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list,
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(
                LazyItems::new(item_count).key(|index| index as u64),
                move |index| {
                    with_key(&(index as u64), || match case {
                        BenchCase::LazyMovableScroll => MovableRowBody(index),
                        BenchCase::LazyMovableHeavy => HeavyMovableRowBody(index),
                        BenchCase::LazyContextMenuScroll => ContextMenuRowBody(index),
                        BenchCase::MovableParentFlip => unreachable!("flip uses its own root"),
                    });
                },
            );
        },
    );
}

#[composable]
fn MovableFlipRoot(handle: Rc<Cell<Option<MutableState<bool>>>>) {
    let side = rememberMutableStateOf(|| true);
    handle.set(Some(side));
    let content = rememberMovableContentOf(|| {
        Box(
            Modifier::empty()
                .size_points(140.0, 80.0)
                .background(Color(0.25, 0.55, 0.85, 1.0)),
            BoxSpec::default(),
            || {
                Box(
                    Modifier::empty().size_points(80.0, 40.0),
                    BoxSpec::default(),
                    || {
                        Text("Movable payload", Modifier::empty(), TextStyle::default());
                    },
                );
            },
        );
    });
    let in_left = side.get();
    let left_content = content.clone();
    Box(
        Modifier::empty().size_points(180.0, 180.0),
        BoxSpec::default(),
        move || {
            if in_left {
                left_content.show();
            }
        },
    );
    Box(
        Modifier::empty().size_points(180.0, 180.0),
        BoxSpec::default(),
        move || {
            if !in_left {
                content.show();
            }
        },
    );
}

#[inline(never)]
fn start_shell(case: BenchCase) -> (AppShell<GraphRenderer>, Driver) {
    let (width, height) = match case {
        BenchCase::LazyMovableHeavy => HEAVY_VIEWPORT,
        BenchCase::LazyMovableScroll
        | BenchCase::LazyContextMenuScroll
        | BenchCase::MovableParentFlip => VIEWPORT,
    };
    let key = location_key(file!(), line!(), column!());
    match case {
        BenchCase::LazyMovableScroll | BenchCase::LazyMovableHeavy => {
            cranpose_ui::set_idle_prefetch_passes(true);
            let handle = Rc::new(Cell::new(None));
            let captured = Rc::clone(&handle);
            let mut shell = AppShell::new_with_size(
                GraphRenderer::default(),
                key,
                move || LazyMovableList(Rc::clone(&captured), case),
                (width, height),
                (width as f32, height as f32),
            );
            shell.update();
            while shell.run_idle_prefetch(None, |_| false) {}
            let scroll_delta = match case {
                BenchCase::LazyMovableHeavy => -(HEAVY_ROW_HEIGHT * HEAVY_ROWS_PER_FRAME),
                _ => -ROW_HEIGHT,
            };
            (
                shell,
                Driver::Lazy {
                    state: handle,
                    scroll_delta,
                },
            )
        }
        BenchCase::LazyContextMenuScroll => {
            cranpose_ui::set_idle_prefetch_passes(true);
            let handle = Rc::new(Cell::new(None));
            let captured = Rc::clone(&handle);
            let mut shell = AppShell::new_with_size(
                GraphRenderer::default(),
                key,
                move || {
                    let captured = Rc::clone(&captured);
                    LiquidTheme(LiquidThemeSpec::system(SchemeMode::Light), move || {
                        LazyMovableList(Rc::clone(&captured), BenchCase::LazyContextMenuScroll);
                    });
                },
                (width, height),
                (width as f32, height as f32),
            );
            shell.update();
            while shell.run_idle_prefetch(None, |_| false) {}
            (
                shell,
                Driver::Lazy {
                    state: handle,
                    scroll_delta: -ROW_HEIGHT,
                },
            )
        }
        BenchCase::MovableParentFlip => {
            cranpose_ui::set_idle_prefetch_passes(false);
            let handle = Rc::new(Cell::new(None));
            let captured = Rc::clone(&handle);
            let mut shell = AppShell::new_with_size(
                GraphRenderer::default(),
                key,
                move || MovableFlipRoot(Rc::clone(&captured)),
                (width, height),
                (width as f32, height as f32),
            );
            shell.update();
            (shell, Driver::Flip(handle))
        }
    }
}

#[inline(never)]
fn steady_frames(shell: &mut AppShell<GraphRenderer>, driver: &Driver, frames: usize) {
    for _ in 0..frames {
        driver.step(shell);
    }
}

fn begin_phase(output: OutputFormat, phase: &str) {
    if output == OutputFormat::SteadyLedger {
        eprintln!("steady_frames: phase {phase}");
    }
}

fn run_case(case: BenchCase, config: &Config) {
    begin_phase(config.output, "start");
    let start_region = Region::new(GLOBAL);
    let (mut shell, driver) = start_shell(case);
    let started = start_region.change();

    begin_phase(config.output, "warmup");
    for _ in 0..config.warmup {
        driver.step(&mut shell);
    }

    begin_phase(config.output, "steady");
    let steady_region = Region::new(GLOBAL);
    let started_at = Instant::now();
    steady_frames(&mut shell, &driver, config.frames);
    let elapsed = started_at.elapsed().as_nanos();
    let steady = steady_region.change();
    begin_phase(config.output, "end");
    black_box(&mut shell);

    match config.output {
        OutputFormat::SteadyLedger => {
            std::mem::forget(shell);
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
            let frames_u128 = config.frames.max(1) as u128;
            println!(
                "{{\"benchmark\":\"movable_content\",\"phase\":\"{}\",\"warmup_frames\":{},\"frames\":{},\"allocations\":{},\"reallocations\":{},\"deallocations\":{},\"bytes_allocated\":{},\"bytes_deallocated\":{},\"elapsed_ns\":{elapsed},\"ns_per_frame\":{}}}",
                case.name(),
                config.warmup,
                config.frames,
                steady.allocations,
                steady.reallocations,
                steady.deallocations,
                steady.bytes_allocated,
                steady.bytes_deallocated,
                elapsed / frames_u128,
            );
        }
    }
}

fn main() {
    let config = Config::from_args();
    match config.case {
        Some(case) => run_case(case, &config),
        None => {
            for case in BenchCase::ALL {
                run_case(case, &config);
            }
        }
    }
}
