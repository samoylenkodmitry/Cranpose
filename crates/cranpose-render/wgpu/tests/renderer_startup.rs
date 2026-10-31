#![allow(
    clippy::print_stderr,
    reason = "the test support prints what a test measured"
)]

#[path = "../src/record_columns.rs"]
#[allow(dead_code, reason = "this binary reads a part of the module")]
mod record_columns;
#[path = "../src/test_support.rs"]
#[allow(dead_code, reason = "this binary reads a part of the module")]
mod shared_test_support;
#[allow(
    dead_code,
    unused_imports,
    reason = "this binary reads a part of the module"
)]
mod support;

use cranpose_render_wgpu::{pipelines_created, pipelines_created_off_frame};
use cranpose_ui_graphics::shader_warm_ups_after;

/// A renderer builds no pipeline on the creating thread, and its compiler
/// builds exactly the warm-ups requested so far, each once, the instant
/// tier's three (shape triangles with and without depth, instant triangles
/// without), and, with no record of a first screen, the eight most screens
/// draw with: text on whole pixels and turned, the layer blit both ways and
/// the turned one, rounded images, shadows and flat solid rects.
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
        requested as u64 + 11,
        "the compiler must build the requested warm-ups, the instant tier and the common eight"
    );
}
