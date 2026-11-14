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

use std::{
    alloc::System,
    hint::black_box,
    sync::Arc,
    time::{Duration, Instant},
};

use cranpose::{launch_args_from_command_line, set_platform_launch_args};
use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use perf_compare::PerfCompareApp;
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const FRAME: Duration = Duration::from_nanos(16_666_667);
const FACES: [&[u8]; 3] = [
    include_bytes!("../../fonts/Roboto-Regular.ttf"),
    include_bytes!("../../fonts/Roboto-Medium.ttf"),
    include_bytes!("../../fonts/Roboto-Bold.ttf"),
];

fn arg(name: &str) -> Option<String> {
    let prefix = format!("--{name}=");
    std::env::args().find_map(|arg| arg.strip_prefix(&prefix).map(str::to_owned))
}

fn arg_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    arg(name)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn frame(shell: &mut AppShell<WgpuRenderer>, (width, height): (u32, u32)) {
    shell.update_after_exact_interval(FRAME);
    let renderer = shell.renderer();
    if let Some(packet) = renderer.build_frame_packet_for_tests(width, height) {
        renderer.return_held_packet_for_tests(packet);
    }
}

#[inline(never)]
fn first_screen(size: (u32, u32), viewport: (f32, f32), density: f32) -> AppShell<WgpuRenderer> {
    let mut shell = AppShell::new_with_size_and_density(
        WgpuRenderer::new(&FACES),
        cranpose_core::location_key(file!(), line!(), column!()),
        PerfCompareApp,
        size,
        viewport,
        density,
    );
    frame(&mut shell, size);
    shell
}

#[inline(never)]
fn steady_frames(shell: &mut AppShell<WgpuRenderer>, size: (u32, u32), frames: usize) {
    for index in 0..frames {
        let start = Instant::now();
        frame(shell, size);
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
    let (width, height) = perf_data::DESKTOP_WINDOW;
    let viewport = (width as f32, height as f32);
    let size = (
        (viewport.0 * density).round() as u32,
        (viewport.1 * density).round() as u32,
    );
    let region = Region::new(GLOBAL);
    let start = Instant::now();
    let mut shell = first_screen(size, viewport, density);
    let elapsed = start.elapsed();
    let stats = region.change();
    println!(
        "{screen} tier {tier} first screen {:.1} ms, {} allocations, {} bytes",
        elapsed.as_secs_f64() * 1e3,
        stats.allocations + stats.reallocations,
        stats.bytes_allocated
    );
    steady_frames(&mut shell, size, frames);
    black_box(&mut shell);
    // The shell's teardown is no part of a start.
    std::mem::forget(shell);
}
