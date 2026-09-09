use std::{cell::RefCell, thread, time::Duration};

use cranpose::Robot;
use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, RepeatMode, StartOffset,
};
use cranpose_app_shell::FpsStats;
use cranpose_core::{rememberMutableStateOf, MutableState};
use cranpose_ui::{composable, Box, BoxSpec, Color, Modifier};

pub(super) const FINISH_HOOK: &str = "perf.finish_calibration";
const SAMPLE_DURATION: Duration = Duration::from_secs(2);

thread_local! {
    static ACTIVE: RefCell<Option<MutableState<bool>>> = const { RefCell::new(None) };
}

pub(super) fn requested(min_fps: f32, max_p95_ms: f32, max_stalls: u32) -> bool {
    min_fps > 0.0 || max_p95_ms > 0.0 || max_stalls != u32::MAX
}

pub(super) fn active(initial: bool) -> bool {
    let state = rememberMutableStateOf(|| initial);
    ACTIVE.with(|slot| *slot.borrow_mut() = Some(state));
    state.get()
}

pub(super) fn finish() -> Result<Option<String>, String> {
    ACTIVE.with(|slot| {
        let state = slot
            .borrow()
            .ok_or("presentation probe is not initialized")?;
        state.set(false);
        Ok(None)
    })
}

#[composable]
#[allow(non_snake_case)]
fn PresentationProbe() {
    let transition = rememberInfiniteTransition("presentation_probe");
    let pulse = transition.animateFloat(
        0.0,
        1.0,
        infiniteRepeatable(
            AnimationSpec::linear(1_379),
            RepeatMode::Restart,
            StartOffset::default(),
        ),
        "probe_color",
    );
    Box(
        Modifier::empty().fill_max_size().background(Color(
            0.1 + pulse.get() * 0.1,
            0.12,
            0.18,
            1.0,
        )),
        BoxSpec::new(),
        || {},
    );
}

pub(super) fn content() {
    PresentationProbe();
}

fn has_headroom(stats: FpsStats, min_fps: f32, max_p95_ms: f32, max_stalls: u32) -> bool {
    stats.interval_count > 0
        && stats.work_fps.is_finite()
        && stats.work_fps > min_fps
        && stats.work_p95_ms.is_finite()
        && (max_p95_ms <= 0.0 || stats.work_p95_ms <= max_p95_ms)
        && stats.work_stalled_50ms_frames <= max_stalls
}

pub(super) fn verify(
    robot: &Robot,
    min_fps: f32,
    max_p95_ms: f32,
    max_stalls: u32,
) -> Result<(), String> {
    robot.wait_for_present_frame()?;
    thread::sleep(SAMPLE_DURATION);
    let first = robot.screenshot()?;
    robot.reset_fps_stats()?;
    thread::sleep(SAMPLE_DURATION);
    let stats = robot.fps_stats()?;
    let render = robot
        .get_render_stats()?
        .ok_or("presentation probe did not render")?;
    let rendered = render.submits > 0 && render.draw_calls > 0;
    let last = robot.screenshot()?;
    let changed =
        first.width == last.width && first.height == last.height && first.pixels != last.pixels;
    let accepted = rendered && changed && has_headroom(stats, min_fps, max_p95_ms, max_stalls);
    println!(
        "PERF_PRESENTATION_CALIBRATION cadence_fps={:.2} work_fps={:.2} work_p95_ms={:.2} intervals={} rendered={} changed={} min_fps={:.2} max_p95_ms={:.2} work_stalls={} max_stalls={} headroom={}",
        stats.fps, stats.work_fps, stats.work_p95_ms, stats.interval_count,
        rendered, changed, min_fps, max_p95_ms, stats.work_stalled_50ms_frames, max_stalls, accepted,
    );
    if !accepted {
        return Err("cannot measure throughput: the animated single-quad presentation probe has no headroom for this budget; use --report-only to measure this surface's cadence".into());
    }
    robot.invoke_app_hook(FINISH_HOOK, "")?;
    robot.wait_for_present_frame()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calibration_rejects_paced_empty_frames_before_gating_a_workload() {
        let paced = FpsStats {
            interval_count: 60,
            work_fps: 60.0,
            work_p95_ms: 16.67,
            ..Default::default()
        };
        assert!(!has_headroom(paced, 120.0, 0.0, u32::MAX));
        assert!(!has_headroom(paced, 0.0, 8.33, u32::MAX));
        assert!(has_headroom(paced, 1.0, 0.0, u32::MAX));
        assert!(!has_headroom(
            FpsStats {
                work_stalled_50ms_frames: 1,
                ..paced
            },
            0.0,
            0.0,
            0
        ));
        assert!(!has_headroom(FpsStats::default(), 1.0, 0.0, u32::MAX));
        assert!(!has_headroom(
            FpsStats {
                work_fps: f32::NAN,
                ..paced
            },
            1.0,
            0.0,
            u32::MAX
        ));
        assert!(!has_headroom(
            FpsStats {
                work_p95_ms: f32::NAN,
                ..paced
            },
            1.0,
            0.0,
            u32::MAX
        ));
        assert!(has_headroom(
            FpsStats {
                work_fps: 240.0,
                work_p95_ms: 4.3,
                ..paced
            },
            120.0,
            8.33,
            u32::MAX
        ));
    }
}
