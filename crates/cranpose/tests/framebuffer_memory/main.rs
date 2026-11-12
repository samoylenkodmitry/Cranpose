//! The heap of a framebuffer host whose screen changes every frame. A
//! microcontroller has a hundred kilobytes or so free: what frames leave
//! behind runs it out of memory.

#![allow(clippy::print_stdout, reason = "a test prints what it measured")]

use std::{alloc::System, ops::Range};

use cranpose::{MemoryHeadroom, SystemTheme, framebuffer::Framebuffer, prelude::*};
use cranpose_animation::{
    AnimationSpec, RepeatMode, StartOffset, infiniteRepeatable, rememberInfiniteTransition,
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[macro_use]
#[path = "../../../coroflow/tests/support/main_thread_test.rs"]
mod main_thread_test;

#[path = "../support/reading.rs"]
mod reading;
#[path = "../support/workload.rs"]
mod workload;

use reading::{ComposedReading, DrawnReading};
use workload::Workload;

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const WIDTH: u32 = 160;
const HEIGHT: u32 = 60;
const FRAME_NANOS: u64 = 16_666_667;
/// Frames that bring the pools and caches to what a steady screen keeps:
/// past the layout passes a text measurement may go unused.
const WARM_UP: Range<u64> = 0..300;
const MEASURED: Range<u64> = 300..1200;
/// What the measured frames may leave on the heap: a pool or cache table
/// that grew once. Frames that kept each text they measured would leave
/// several times this.
const LEFT_BEHIND_BOUND: isize = 32 * 1024;
// The bars a canvas draws, enough that a second copy of what they draw
// would hold far more than `REDRAW_BOUND`.
const BARS: usize = 200;
// A count of bars where lists that double fill their storage, and what one
// bar more may hold on a host with little memory left: the bar and the room
// a trimmed list keeps, an eighth of its items, not room for as many more,
// which held 10 KB more.
const FULL_BARS: usize = 64;
const BAR_BOUND: isize = 4 * 1024;
// What a canvas that draws again every frame may hold more than one that
// drew once: lists that grew once, the reach of each item and the areas a
// frame changes, 16 bytes a bar each. The 200 bars themselves hold about
// 65 KB, so a copy of what the canvas draws holds several times this.
const REDRAW_BOUND: isize = 12 * 1024;

fn run(framebuffer: &mut Framebuffer, frames: Range<u64>, pixels: &mut [u8]) {
    let mut areas = Vec::new();
    for frame in frames {
        if framebuffer.tick(frame * FRAME_NANOS) {
            framebuffer.changed_areas(&mut areas);
            for area in &areas {
                let len = area.width as usize * area.height as usize * 4;
                framebuffer.draw_area(*area, &mut pixels[..len]);
            }
        }
        framebuffer.run_pending_tasks();
    }
}

// The bytes a framebuffer of `content` holds after the `WARM_UP` frames, its
// host reporting `memory` left.
fn held(content: impl Fn() + 'static, memory: MemoryHeadroom) -> isize {
    let mut pixels = vec![0; WIDTH as usize * HEIGHT as usize * 4];
    let region = Region::new(GLOBAL);
    let mut framebuffer = Framebuffer::new(WIDTH, HEIGHT, 1.0, SystemTheme::Dark, &[], content)
        .expect("a framebuffer");
    framebuffer.set_active(true);
    if memory == MemoryHeadroom::Tight {
        framebuffer.report_memory(0, 1);
    }
    run(&mut framebuffer, WARM_UP, &mut pixels);
    let change = region.change();
    change.bytes_allocated as isize - change.bytes_deallocated as isize
}

// `count` bars in rows that a canvas draws, shifted along as an animation
// runs when `moving`: each frame then draws them all again.
#[composable]
fn Bars(moving: bool, count: usize) {
    let shift = rememberInfiniteTransition("bars").animateFloat(
        0.0,
        8.0,
        infiniteRepeatable(
            AnimationSpec::linear(1_000),
            RepeatMode::Reverse,
            StartOffset::default(),
        ),
        "shift",
    );
    Box(
        Modifier::empty()
            .width(WIDTH as f32)
            .height(HEIGHT as f32)
            .draw_behind(move |scope| {
                let offset = if moving { shift.value() } else { 0.0 };
                for bar in 0..count {
                    let rect = Rect {
                        x: offset + (bar % 40) as f32 * 4.0,
                        y: (bar / 40) as f32 * 12.0,
                        width: 3.0,
                        height: 10.0,
                    };
                    scope.draw_rect_at(rect, Brush::solid(Color::WHITE));
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

/// The bytes that `MEASURED` frames of `content` leave on the heap.
fn left_behind(content: impl Fn() + 'static) -> isize {
    let mut framebuffer = Framebuffer::new(WIDTH, HEIGHT, 1.0, SystemTheme::Dark, &[], content)
        .expect("a framebuffer");
    framebuffer.set_active(true);
    let mut pixels = vec![0; WIDTH as usize * HEIGHT as usize * 4];
    run(&mut framebuffer, WARM_UP, &mut pixels);
    let region = Region::new(GLOBAL);
    run(&mut framebuffer, MEASURED, &mut pixels);
    let change = region.change();
    change.bytes_allocated as isize - change.bytes_deallocated as isize
}

/// A number that changes every frame, as a clock or a sensor reading does, composed in a
/// `Text` or drawn in a draw scope, leaves no memory behind: the text pools and caches keep
/// what the screen shows and what recent frames used. Nor does a busier screen, whose list
/// scrolls and whose panel comes and goes. A canvas drawn again every frame holds what one
/// drawn once does, and with little memory left one bar more holds about that bar. One
/// test: the allocator counts every thread's memory.
fn screens_that_change_every_frame_leave_no_memory_behind() {
    let composed = left_behind(|| ComposedReading(TextStyle::default()));
    let drawn = left_behind(|| DrawnReading(Color::WHITE, DrawTextStyle::default().font_size));
    let busy = left_behind(|| Workload(TextStyle::default()));
    println!("left behind: composed {composed} bytes, drawn {drawn} bytes, busy {busy} bytes");
    assert!(composed < LEFT_BEHIND_BOUND, "composed: {composed} bytes");
    assert!(drawn < LEFT_BEHIND_BOUND, "drawn: {drawn} bytes");
    assert!(busy < LEFT_BEHIND_BOUND, "busy: {busy} bytes");
    // A first host fills the thread's caches and pools, which the hosts
    // measured after it then find alike.
    let _ = held(|| Bars(true, BARS), MemoryHeadroom::Ample);
    let still = held(|| Bars(false, BARS), MemoryHeadroom::Ample);
    let moving = held(|| Bars(true, BARS), MemoryHeadroom::Ample);
    println!("held: still bars {still} bytes, moving bars {moving} bytes");
    assert!(
        moving - still < REDRAW_BOUND,
        "moving bars hold {} bytes more",
        moving - still
    );
    let full = held(|| Bars(false, FULL_BARS), MemoryHeadroom::Tight);
    let past_full = held(|| Bars(false, FULL_BARS + 1), MemoryHeadroom::Tight);
    println!("held with little memory: {FULL_BARS} bars {full} bytes, one more {past_full} bytes");
    assert!(
        past_full - full < BAR_BOUND,
        "one bar more holds {} bytes more",
        past_full - full
    );
}

main_thread_test!(screens_that_change_every_frame_leave_no_memory_behind);
