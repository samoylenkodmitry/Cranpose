use cranpose_app_shell::AppShell;
use cranpose_core::{LaunchedEffectAsync, location_key, rememberMutableStateOf};
use cranpose_ui::{Box, BoxSpec, Color, Modifier, composable};

use crate::support;

const SIZE: u32 = 64;

/// A layer turned by a state that a frame-clock loop writes once a frame,
/// the way an animation driven by `next_frame` does.
#[composable]
fn FrameDrivenLayer() {
    let seconds = rememberMutableStateOf(|| 0.0f32);
    LaunchedEffectAsync((), move |scope| {
        std::boxed::Box::pin(async move {
            let clock = scope.runtime().frame_clock();
            while scope.is_active() {
                let now = clock.next_frame().await;
                if !scope.is_active() {
                    break;
                }
                seconds.set(now as f32 / 1e6);
            }
        })
    });
    Box(
        Modifier::empty()
            .size_points(SIZE as f32 / 2.0, SIZE as f32 / 2.0)
            .graphics_layer_block(move |layer| layer.rotation_z = seconds.get() % 360.0)
            .background(Color(0.2, 0.5, 0.8, 1.0)),
        BoxSpec::default(),
        || {},
    );
}

#[test]
fn a_layer_read_state_written_every_frame_changes_every_frame() {
    let Ok((_lock, renderer)) = support::headless_renderer_parts() else {
        eprintln!("skipping (headless WGPU init failed)");
        return;
    };
    let mut shell = AppShell::new(
        renderer,
        location_key(file!(), line!(), column!()),
        FrameDrivenLayer,
    );
    shell.set_viewport(SIZE as f32, SIZE as f32);
    shell.set_buffer_size(SIZE, SIZE);
    for _ in 0..4 {
        shell.update();
    }
    let changed: Vec<bool> = (1..=12u64)
        .map(|frame| {
            let result = shell.update_at_frame_time_nanos(1_000_000_000 + frame * 8_333_333);
            result.visual_changed
        })
        .collect();
    assert!(
        changed.iter().all(|changed| *changed),
        "a frame whose clock callback writes the state a layer reads must redraw that \
         layer in the same frame: {changed:?}"
    );
}
