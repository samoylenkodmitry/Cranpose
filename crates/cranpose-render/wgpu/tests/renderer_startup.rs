use cranpose_render_wgpu::{pipelines_created, pipelines_created_off_frame};
use cranpose_ui_graphics::shader_warm_ups_after;

use crate::support;

/// A renderer compiles nothing on speculation: creating one builds no
/// pipeline on the creating thread, and its compiler builds exactly the
/// warm-ups requested so far, each once.
#[test]
fn a_new_renderer_builds_only_the_requested_warm_ups() {
    let (_lock, (created, created_off_frame, requested), _renderer) =
        support::headless_renderer_parts_configured(|| {
            support::wait_for_background_compiler_idle();
            (
                pipelines_created(),
                pipelines_created_off_frame(),
                shader_warm_ups_after(0).len(),
            )
        })
        .expect("GPU required for the startup contract");
    assert_eq!(
        pipelines_created(),
        created,
        "creating a renderer must not build pipelines no frame has asked for"
    );
    support::wait_for_background_compiler_idle();
    assert_eq!(
        pipelines_created_off_frame() - created_off_frame,
        requested as u64,
        "the compiler must build the requested warm-ups and nothing else"
    );
}
