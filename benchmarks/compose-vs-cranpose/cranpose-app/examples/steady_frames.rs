//! The allocator calls of a bench layout's start and steady frames, through
//! the wgpu renderer's producer side without a GPU: the work a phone's UI
//! thread does for each frame. `scripts/perf/steady_ledger.sh` runs it for
//! every bench layout and compares the counts with their budgets.
//!
//! It is an example, not a bench: cargo builds benches with `panic =
//! "unwind"` whatever the profile says, and their code moves more bytes
//! than the app's `panic = "abort"` release build does. An example builds
//! with the app's release profile.
//!
//! The layout comes from the app's own command line, as on a desktop
//! (`--scenario=feed --speed=12000 --fcols=4 --ui=0.3`), in the 1280 by 820
//! point window of the desktop apps at `--density` (default 2). The run
//! starts the app with its first frame in `start`, steps `--warmup` frames,
//! then counts `--frames` frames in `steady_frames`; the gate's script sets
//! both. It prints one line of counts: the start's `malloc`, `realloc` and
//! `free` calls and the nodes it left, then those calls of the counted
//! frames.
//!
//! Under valgrind, the stacks that hold `start` or `steady_frames` place a
//! copy or a block in its phase. Each phase also begins with a
//! `steady_frames: phase NAME` line on stderr, which places the allocator
//! calls that valgrind traces there in their phase.

use std::{alloc::System, hint::black_box, sync::Arc};

use cranpose::{launch_args_from_command_line, set_platform_launch_args};
use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[path = "../benches/support/desktop_window.rs"]
mod desktop_window;

use desktop_window::{DesktopWindow, arg, arg_or};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

fn begin(phase: &str) {
    eprintln!("steady_frames: phase {phase}");
}

fn frames_arg(name: &str) -> usize {
    arg(name)
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| panic!("--{name}=N is needed: scripts/perf/steady_ledger.sh sets it"))
}

#[inline(never)]
fn start(window: DesktopWindow) -> AppShell<WgpuRenderer> {
    window.start()
}

#[inline(never)]
fn steady_frames(shell: &mut AppShell<WgpuRenderer>, window: DesktopWindow, frames: usize) {
    for _ in 0..frames {
        window.frame(shell);
    }
}

fn main() {
    let warmup = frames_arg("warmup");
    let frames = frames_arg("frames");
    let density: f32 = arg_or("density", 2.0);
    set_platform_launch_args(Arc::new(launch_args_from_command_line(
        std::env::args().skip(1),
        false,
    )));
    let window = DesktopWindow::new(density);
    begin("start");
    let region = Region::new(GLOBAL);
    let mut shell = start(window);
    let started = region.change();
    begin("warmup");
    let nodes = shell.debug_runtime_leak_stats().applier_stats.nodes_len;
    for _ in 0..warmup {
        window.frame(&mut shell);
    }
    begin("steady");
    let region = Region::new(GLOBAL);
    steady_frames(&mut shell, window, frames);
    let steady = region.change();
    begin("end");
    black_box(&mut shell);
    // The shell's teardown is no part of the counts.
    std::mem::forget(shell);
    println!(
        "steady_frames warmup={warmup} frames={frames} start_malloc={} start_realloc={} \
         start_free={} nodes={nodes} malloc={} realloc={} free={}",
        started.allocations,
        started.reallocations,
        started.deallocations,
        steady.allocations,
        steady.reallocations,
        steady.deallocations
    );
}
