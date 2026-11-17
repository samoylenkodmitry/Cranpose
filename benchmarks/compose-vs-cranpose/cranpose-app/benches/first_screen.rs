//! What the bench app does between its launch and its first frame: it
//! composes the screen, measures and places it, builds its scene and the
//! first frame the wgpu renderer hands its GPU, here without a GPU. The still
//! wall at tier 20 composes about 59,000 tiles inside a `BoxWithConstraints`
//! while it measures them, so a cost per node of composition, subcomposition,
//! layout or scene building shows large here.
//!
//! `--scenario=wall` (default) or `--scenario=gauntlet` names the screen, as
//! on the app's command line, and `--tier` its tier (default 20 for the wall
//! and 16 for the gauntlet, the tiers of the nightly), in the 1280 by 820
//! point window of the desktop apps at `--density` (default 2). The run
//! prints the start's time, its allocations and the bytes they asked for,
//! then the time of each of `--frames` frames after it (default 3).
//! `cargo bench --bench first_screen -- --scenario=gauntlet` runs it. Under
//! callgrind,
//! `--toggle-collect=*first_screen*` counts the start alone and
//! `--toggle-collect=*steady_frames*` the frames after it.

use std::{alloc::System, hint::black_box, sync::Arc, time::Instant};

use cranpose::{launch_args_from_command_line, set_platform_launch_args};
use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[path = "support/desktop_window.rs"]
mod desktop_window;

use desktop_window::{DesktopWindow, arg, arg_or};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

#[inline(never)]
fn first_screen(window: DesktopWindow) -> AppShell<WgpuRenderer> {
    window.start()
}

#[inline(never)]
fn steady_frames(shell: &mut AppShell<WgpuRenderer>, window: DesktopWindow, frames: usize) {
    for index in 0..frames {
        let start = Instant::now();
        window.frame(shell);
        println!(
            "frame {} {:.2} ms",
            index + 1,
            start.elapsed().as_secs_f64() * 1e3
        );
    }
}

fn main() {
    let screen = arg("scenario").unwrap_or_else(|| "wall".to_owned());
    let default_tier = if screen == "gauntlet" { 16 } else { 20 };
    let tier: usize = arg_or("tier", default_tier);
    let frames: usize = arg_or("frames", 3);
    let density: f32 = arg_or("density", 2.0);
    set_platform_launch_args(Arc::new(launch_args_from_command_line(
        [format!("--scenario={screen}"), format!("--tier={tier}")],
        false,
    )));
    let window = DesktopWindow::new(density);
    let region = Region::new(GLOBAL);
    let start = Instant::now();
    let mut shell = first_screen(window);
    let elapsed = start.elapsed();
    let stats = region.change();
    println!(
        "{screen} tier {tier} first screen {:.1} ms, {} allocations, {} bytes",
        elapsed.as_secs_f64() * 1e3,
        stats.allocations + stats.reallocations,
        stats.bytes_allocated
    );
    steady_frames(&mut shell, window, frames);
    black_box(&mut shell);
    // The shell's teardown is no part of a start.
    std::mem::forget(shell);
}
