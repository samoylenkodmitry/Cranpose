use std::time::Duration;

use cranpose::{
    widgets::{BasicTextField, Box as CBox, BoxSpec},
    AppLauncher, Color, Modifier, Robot, RobotScreenshot, Size,
};
use cranpose_foundation::text::TextFieldState;
use cranpose_ui::text::TextStyle;

const CYCLES: usize = 30;
const SWEEPS_PER_CYCLE: usize = 10;
const SWEEP_STEPS: usize = 6;
const FIELD_X: f32 = 50.0;
const FIELD_Y: f32 = 160.0;
const FIELD_WIDTH: f32 = 500.0;
const FIELD_HEIGHT: f32 = 72.0;

fn cycle_count() -> usize {
    std::env::var("CRANPOSE_CYCLE_STABILITY_CYCLES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(CYCLES)
}

struct CycleSample {
    drag_work_avg_ms: f32,
    drag_work_p95_ms: f32,
    work_avg_ms: f32,
    work_p95_ms: f32,
    pass_count: u32,
    effect_applies: u32,
    isolated_layer_renders: u32,
    layer_cache_size: u32,
    layer_cache_bytes: u64,
    offscreen_pool_size: u32,
    offscreen_pool_bytes: u64,
    text_cache_size: u32,
    nodes_len: usize,
    live_node_heap_bytes: usize,
    slot_table_heap_bytes: usize,
    slot_group_count: usize,
    observer_scopes_len: usize,
    observed_state_count: usize,
    recompose_scopes_len: usize,
    frame_callbacks_len: usize,
    local_tasks_len: usize,
    tasks_len: usize,
    ui_conts_len: usize,
    state_cells_len: usize,
}

fn main() {
    env_logger::init();
    println!("=== Text Handle Cycle Stability ===\n");

    AppLauncher::new()
        .with_title("Handle Cycle Stability")
        .with_size(600, 400)
        .with_headless(true)
        .with_test_driver(move |robot| {
            const TEST_TIMEOUT_SECS: u64 = 400;
            std::thread::spawn(|| {
                std::thread::sleep(Duration::from_secs(TEST_TIMEOUT_SECS));
                println!("\n✗ Test timed out after {TEST_TIMEOUT_SECS} seconds");
                std::process::exit(1);
            });

            std::thread::sleep(Duration::from_millis(300));

            let left_x = FIELD_X + 12.0;
            let right_x = FIELD_X + FIELD_WIDTH - 12.0;
            let line_y = FIELD_Y + FIELD_HEIGHT * 0.5;

            let cycles = cycle_count();
            let mut samples: Vec<CycleSample> = Vec::with_capacity(cycles);
            for _cycle in 0..cycles {
                let drag = run_cycle(&robot, left_x, right_x, line_y);
                samples.push(sample(&robot, drag, samples.len()));
                if samples.len() == 1 || samples.len() == cycles {
                    dump_all_stats(&robot, samples.len() - 1);
                }
            }

            println!(
                "\n{:>3} {:>8} {:>8} {:>8} {:>8} {:>5} {:>6} {:>6} {:>6} {:>11} {:>5} {:>11} {:>6} {:>7} {:>11} {:>11} {:>8} {:>7} {:>7} {:>7} {:>6} {:>6} {:>6} {:>7} {:>7}",
                "cyc", "drag_avg", "drag_p95", "work_avg", "work_p95", "pass", "effect",
                "isolay", "lcache", "lcache_B", "pool", "pool_B", "tcache", "nodes",
                "node_heap", "slot_heap", "groups", "obs_sc", "obs_st", "rscopes",
                "frcb", "ltask", "task", "uicont", "cells"
            );
            for (i, s) in samples.iter().enumerate() {
                println!(
                    "{:>3} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>5} {:>6} {:>6} {:>6} {:>11} {:>5} {:>11} {:>6} {:>7} {:>11} {:>11} {:>8} {:>7} {:>7} {:>7} {:>6} {:>6} {:>6} {:>7} {:>7}",
                    i,
                    s.drag_work_avg_ms,
                    s.drag_work_p95_ms,
                    s.work_avg_ms,
                    s.work_p95_ms,
                    s.pass_count,
                    s.effect_applies,
                    s.isolated_layer_renders,
                    s.layer_cache_size,
                    s.layer_cache_bytes,
                    s.offscreen_pool_size,
                    s.offscreen_pool_bytes,
                    s.text_cache_size,
                    s.nodes_len,
                    s.live_node_heap_bytes,
                    s.slot_table_heap_bytes,
                    s.slot_group_count,
                    s.observer_scopes_len,
                    s.observed_state_count,
                    s.recompose_scopes_len,
                    s.frame_callbacks_len,
                    s.local_tasks_len,
                    s.tasks_len,
                    s.ui_conts_len,
                    s.state_cells_len,
                );
            }

            let early = &samples[1];
            let late = &samples[cycles - 1];
            let structural: [(&str, f64, f64, f64); 7] = [
                ("pass_count", early.pass_count as f64, late.pass_count as f64, 1.30),
                (
                    "effect_applies",
                    early.effect_applies as f64,
                    late.effect_applies as f64,
                    1.30,
                ),
                (
                    "isolated_layer_renders",
                    early.isolated_layer_renders as f64,
                    late.isolated_layer_renders as f64,
                    1.30,
                ),
                (
                    "layer_cache_size",
                    early.layer_cache_size as f64,
                    late.layer_cache_size as f64,
                    1.50,
                ),
                (
                    "nodes_len",
                    early.nodes_len as f64,
                    late.nodes_len as f64,
                    1.10,
                ),
                (
                    "observer_scopes_len",
                    early.observer_scopes_len as f64,
                    late.observer_scopes_len as f64,
                    1.10,
                ),
                (
                    "recompose_scopes_len",
                    early.recompose_scopes_len as f64,
                    late.recompose_scopes_len as f64,
                    1.10,
                ),
            ];
            let runtime_queues: [(&str, f64, f64); 5] = [
                (
                    "frame_callbacks_len",
                    early.frame_callbacks_len as f64,
                    late.frame_callbacks_len as f64,
                ),
                (
                    "local_tasks_len",
                    early.local_tasks_len as f64,
                    late.local_tasks_len as f64,
                ),
                ("tasks_len", early.tasks_len as f64, late.tasks_len as f64),
                (
                    "ui_conts_len",
                    early.ui_conts_len as f64,
                    late.ui_conts_len as f64,
                ),
                (
                    "state_cells_len",
                    early.state_cells_len as f64,
                    late.state_cells_len as f64,
                ),
            ];
            let mut failures = Vec::new();
            for (name, early_v, late_v, tolerance) in structural {
                let floor = early_v.max(8.0);
                let allowed = (floor * tolerance).ceil();
                if late_v > allowed {
                    failures.push(format!(
                        "{name} grew {early_v:.0} → {late_v:.0} (allowed {allowed:.0})"
                    ));
                }
            }
            for (name, early_v, late_v) in runtime_queues {
                if late_v > early_v.max(8.0) * 1.25 {
                    failures.push(format!("{name} grew {early_v:.0} → {late_v:.0}"));
                }
            }
            let avg3 = |pick: fn(&CycleSample) -> f32, window: &[CycleSample]| {
                window.iter().map(|s| pick(s) as f64).sum::<f64>() / window.len() as f64
            };
            let early_drag = avg3(|s| s.drag_work_avg_ms, &samples[1..4]).max(0.3);
            let late_drag = avg3(|s| s.drag_work_avg_ms, &samples[cycles - 3..]);
            if late_drag > early_drag * 1.4 + 0.15 {
                failures.push(format!(
                    "drag work_avg_ms grew {early_drag:.2} → {late_drag:.2}"
                ));
            }
            let early_work = avg3(|s| s.work_avg_ms, &samples[1..4]).max(0.3);
            let late_work = avg3(|s| s.work_avg_ms, &samples[cycles - 3..]);
            if late_work > early_work * 1.4 + 0.15 {
                failures.push(format!(
                    "idle work_avg_ms grew {early_work:.2} → {late_work:.2}"
                ));
            }
            assert!(
                failures.is_empty(),
                "per-cycle accumulation detected:\n  {}",
                failures.join("\n  ")
            );

            println!("\n✓ PASS: {cycles} select/sweep/release cycles stayed flat");
            let _ = robot.exit();
        })
        .run(HandleCycleFixture);
}

#[allow(non_snake_case)]
#[cranpose::composable]
fn HandleCycleFixture() {
    let state = cranpose_core::remember(|| {
        TextFieldState::new("Type here with enough text for repeated selection handle sweeps")
    })
    .with(TextFieldState::clone);
    CBox(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.08, 0.10, 0.18, 1.0)),
        BoxSpec::default(),
        move || {
            BasicTextField(
                state,
                Modifier::empty()
                    .absolute_offset(FIELD_X, FIELD_Y)
                    .size(Size::new(FIELD_WIDTH, FIELD_HEIGHT))
                    .padding(12.0)
                    .background(Color(0.15, 0.18, 0.25, 1.0)),
                TextStyle::default(),
            );
        },
    );
}

fn run_cycle(robot: &Robot, left_x: f32, right_x: f32, line_y: f32) -> (f32, f32) {
    let _ = robot.click((left_x + right_x) * 0.5, line_y);
    let _ = robot.wait_for_present_frame();

    let _ = robot.mouse_move(right_x, line_y);
    let _ = robot.mouse_down();
    let _ = robot.wait_for_present_frame();
    for step in 1..=4 {
        let t = step as f32 / 4.0;
        let _ = robot.mouse_move(right_x + (left_x - right_x) * t, line_y);
        let _ = robot.wait_for_present_frame();
    }
    let _ = robot.mouse_up();
    let _ = robot.wait_for_present_frame();

    let shot = robot.screenshot().expect("handle-grab frame");
    let grab =
        lower_handle_center(&shot, (left_x, line_y, right_x - left_x)).unwrap_or((left_x, line_y));
    let _ = robot.mouse_move(grab.0, grab.1);
    let _ = robot.mouse_down();
    let _ = robot.wait_for_present_frame();
    let _ = robot.reset_fps_stats();
    for sweep in 0..SWEEPS_PER_CYCLE {
        let (from, to) = if sweep % 2 == 0 {
            (grab.0, left_x)
        } else {
            (left_x, right_x)
        };
        for step in 1..=SWEEP_STEPS {
            let t = step as f32 / SWEEP_STEPS as f32;
            let _ = robot.mouse_move(from + (to - from) * t, grab.1);
            let _ = robot.wait_for_present_frame();
        }
    }
    let drag_fps = robot.fps_stats().expect("drag-window fps stats");
    let _ = robot.mouse_up();
    let _ = robot.click((left_x + right_x) * 0.5, line_y);
    let _ = robot.pump_frames(4);
    (drag_fps.work_avg_ms, drag_fps.work_p95_ms)
}

fn sample(robot: &Robot, drag: (f32, f32), cycle: usize) -> CycleSample {
    let _ = robot.reset_fps_stats();
    let _ = robot.pump_frames(30);
    let fps = robot.fps_stats().expect("fps stats");
    let render = robot
        .get_render_stats()
        .expect("render stats")
        .expect("renderer publishes frame stats");
    let leak = robot
        .get_runtime_leak_debug_stats()
        .expect("runtime leak stats");
    println!(
        "cycle {cycle:>2}: drag {:.2}ms idle {:.2}ms | passes {} effects {} layers {} | nodes {} obs {} frcb {} cells {}",
        drag.0,
        fps.work_avg_ms,
        render.pass_count,
        render.effect_applies,
        render.isolated_layer_renders,
        leak.applier_stats.nodes_len,
        leak.observer_stats.scopes_len,
        leak.runtime_stats.frame_callbacks_len,
        leak.state_arena_stats.cells_len,
    );
    CycleSample {
        drag_work_avg_ms: drag.0,
        drag_work_p95_ms: drag.1,
        work_avg_ms: fps.work_avg_ms,
        work_p95_ms: fps.work_p95_ms,
        pass_count: render.pass_count,
        effect_applies: render.effect_applies,
        isolated_layer_renders: render.isolated_layer_renders,
        layer_cache_size: render.layer_cache_size,
        layer_cache_bytes: render.layer_cache_bytes,
        offscreen_pool_size: render.offscreen_pool_size,
        offscreen_pool_bytes: render.offscreen_pool_bytes,
        text_cache_size: render.text_cache_size,
        nodes_len: leak.applier_stats.nodes_len,
        live_node_heap_bytes: leak.live_node_heap_bytes,
        slot_table_heap_bytes: leak.slot_table_heap_bytes,
        slot_group_count: leak.slot_stats.group_count,
        observer_scopes_len: leak.observer_stats.scopes_len,
        observed_state_count: leak.observer_stats.observed_state_count,
        recompose_scopes_len: leak.recompose_scope_stats.len,
        frame_callbacks_len: leak.runtime_stats.frame_callbacks_len,
        local_tasks_len: leak.runtime_stats.local_tasks_len,
        tasks_len: leak.runtime_stats.tasks_len,
        ui_conts_len: leak.runtime_stats.ui_conts_len,
        state_cells_len: leak.state_arena_stats.cells_len,
    }
}

fn dump_all_stats(robot: &Robot, cycle: usize) {
    if let Ok(leak) = robot.get_runtime_leak_debug_stats() {
        println!("=== cycle {cycle} runtime stats ===\n{leak:#?}");
    }
    if let Ok(alloc) = robot.get_render_cpu_allocation_stats() {
        println!("=== cycle {cycle} render cpu allocations ===\n{alloc:#?}");
    }
}

fn lower_handle_center(shot: &RobotScreenshot, band: (f32, f32, f32)) -> Option<(f32, f32)> {
    let (sx, sy) = (
        shot.width as f32 / shot.logical_width.max(1.0),
        shot.height as f32 / shot.logical_height.max(1.0),
    );
    let (x, line_y, width) = band;
    let left = (x.max(0.0) * sx) as usize;
    let right = (((x + width) * sx) as usize).min(shot.width as usize);
    let top = ((line_y * sy) as usize).min(shot.height as usize);
    let bottom = (((line_y + 40.0) * sy) as usize).min(shot.height as usize);

    let is_blue = |px: usize, py: usize| {
        let i = (py * shot.width as usize + px) * 4;
        let (r, g, b) = (shot.pixels[i], shot.pixels[i + 1], shot.pixels[i + 2]);
        b > 170 && b.saturating_sub(r) > 55 && b.saturating_sub(g) > 25
    };
    let max_y = (top..bottom)
        .rev()
        .find(|&py| (left..right).any(|px| is_blue(px, py)))?;
    let band_top = max_y.saturating_sub((4.0 * sy).ceil() as usize);
    let mut sum_x = 0usize;
    let mut sum_y = 0usize;
    let mut count = 0usize;
    for py in band_top..=max_y {
        for px in left..right {
            if is_blue(px, py) {
                sum_x += px;
                sum_y += py;
                count += 1;
            }
        }
    }
    (count > 0).then(|| {
        (
            sum_x as f32 / count as f32 / sx,
            sum_y as f32 / count as f32 / sy,
        )
    })
}
