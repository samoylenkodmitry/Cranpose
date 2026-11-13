// How many frames a producer hands the present thread at once: two while
// the window's images hold a frame to build on, one while a frame draws
// whole for images that hold none.

use std::time::{Duration, Instant};

use cranpose_render_common::Renderer;
use cranpose_render_wgpu::{
    CancelReason, InlinePresentRuntime, PresentOutcome, PublishOutcome, TestImages, WgpuRenderer,
};

use crate::present_runtime_contract::{
    HEIGHT, WIDTH, direct_graph, drain_outcomes, gpu_target, publish, replace_device_then_publish,
    shadowed_child_graph, surface_config, threaded_parts,
};

// The renderer on an inline runtime with a test swapchain of `images`
// attached; `None` when the machine has no headless device.
fn attached(
    name: &str,
    images: TestImages,
) -> Option<(
    std::sync::MutexGuard<'static, ()>,
    WgpuRenderer,
    InlinePresentRuntime,
)> {
    let (lock, mut renderer, target) = match threaded_parts(wgpu::TextureFormat::Bgra8UnormSrgb) {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping {name}: headless WGPU init failed: {err}");
            return None;
        }
    };
    let mut runtime = renderer.init_gpu_inline_for_tests(target);
    attach(&mut renderer, &mut runtime, images);
    Some((lock, renderer, runtime))
}

fn attach(renderer: &mut WgpuRenderer, runtime: &mut InlinePresentRuntime, images: TestImages) {
    let ack = renderer
        .present_thread()
        .and_then(|thread| thread.send_attach_offscreen_unacked_for_tests(WIDTH, HEIGHT, images))
        .expect("inline runtime must accept controls");
    runtime.pump();
    ack.try_recv().expect("the test swapchain is attached");
}

// Draws changing frames until the images hold a frame to build on, as a
// window's do past its start, so a frame goes while another is in flight.
// Leaves no frame in flight and the shadowed child described, so a frame of
// the direct graph draws next.
fn hold_frames(renderer: &mut WgpuRenderer, runtime: &mut InlinePresentRuntime) {
    let graphs = [direct_graph(), shadowed_child_graph()];
    let deadline = Instant::now() + Duration::from_secs(30);
    for frame in 0.. {
        renderer.scene_mut().describe(&graphs[frame % graphs.len()]);
        assert_eq!(
            renderer.publish_frame(WIDTH, HEIGHT),
            PublishOutcome::Published,
            "with no frame in flight every frame goes"
        );
        let overlaps = renderer.has_frame_credit();
        runtime.pump();
        drain_outcomes(renderer);
        if overlaps && frame % graphs.len() == 1 {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the images never held a frame to build on"
        );
    }
}

#[test]
fn a_frame_drawn_whole_for_images_that_hold_nothing_goes_alone() {
    let Some((_lock, mut renderer, mut runtime)) = attached("whole frames", TestImages::Fresh)
    else {
        return;
    };
    renderer.scene_mut().describe(&direct_graph());
    for frame in 1..=3 {
        assert_eq!(
            renderer.publish_frame(WIDTH, HEIGHT),
            PublishOutcome::Published
        );
        assert!(
            !renderer.has_frame_credit(),
            "images that hold nothing take each frame whole, and a whole frame waits for the \
             frame in flight: one scene then holds every whole frame"
        );
        assert_eq!(
            renderer.publish_frame(WIDTH, HEIGHT),
            PublishOutcome::NoCredit
        );
        assert_eq!(
            renderer.last_published_frame_id(),
            frame,
            "a frame that waits is not built"
        );
        runtime.pump();
        assert_eq!(
            drain_outcomes(&mut renderer),
            vec![(frame, PresentOutcome::Presented)]
        );
        assert!(renderer.has_frame_credit(), "with none in flight it goes");
    }
}

#[test]
fn depth_one_credit_gates_publish_before_lowering() {
    let Some((_lock, mut renderer, mut runtime)) =
        attached("depth-one credit", TestImages::Kept(3))
    else {
        return;
    };
    hold_frames(&mut renderer, &mut runtime);
    let first = renderer.last_published_frame_id() + 1;
    renderer.scene_mut().describe(&direct_graph());
    assert!(renderer.has_frame_credit());
    publish(&mut renderer);
    assert_eq!(renderer.last_published_frame_id(), first);
    assert!(
        renderer.has_frame_credit(),
        "one rendering plus one waiting: the second credit is what makes \
         the producer and present stages overlap"
    );
    renderer.scene_mut().describe(&shadowed_child_graph());
    publish(&mut renderer);
    assert_eq!(renderer.last_published_frame_id(), first + 1);
    assert!(
        !renderer.has_frame_credit(),
        "two packets in flight is the bound; the producer stalls here so \
         it can never run away from the screen"
    );
    assert_eq!(
        renderer.publish_frame(WIDTH, HEIGHT),
        PublishOutcome::NoCredit,
        "both slots are occupied"
    );
    assert_eq!(
        renderer.last_published_frame_id(),
        first + 1,
        "a NoCredit publish must not build a packet: backpressure lands \
         before the lowering work"
    );

    runtime.pump();
    runtime.pump();
    assert_eq!(
        drain_outcomes(&mut renderer),
        vec![
            (first, PresentOutcome::Presented),
            (first + 1, PresentOutcome::Presented)
        ],
        "the offscreen surrogate must render both packets, in publish order"
    );
    assert!(
        renderer.has_frame_credit(),
        "drained returns free the publish credit"
    );
    publish(&mut renderer);
    assert_eq!(renderer.last_published_frame_id(), first + 2);
}

#[test]
fn a_returned_frame_frees_a_credit_it_reports_until_drained() {
    let Some((_lock, mut renderer, mut runtime)) = attached("returned credit", TestImages::Kept(3))
    else {
        return;
    };
    hold_frames(&mut renderer, &mut runtime);
    assert!(!renderer.frame_credit_returned(), "every return is drained");
    for graph in [direct_graph(), shadowed_child_graph()] {
        renderer.scene_mut().describe(&graph);
        assert_eq!(
            renderer.publish_frame(WIDTH, HEIGHT),
            PublishOutcome::Published
        );
    }
    assert!(!renderer.has_frame_credit());
    assert!(
        !renderer.frame_credit_returned(),
        "a packet not yet rendered holds its credit"
    );

    runtime.pump();
    assert!(
        !renderer.has_frame_credit(),
        "a return frees its credit only once drained"
    );
    assert!(
        renderer.frame_credit_returned(),
        "the rendered packet's return is waiting to be drained"
    );
    assert!(renderer.drain_present_returns() >= 1);
    assert!(renderer.has_frame_credit());
    assert!(
        !renderer.frame_credit_returned(),
        "a drained return is no longer reported"
    );
}

#[test]
fn a_resize_waits_for_nothing_and_the_frames_after_it_draw_at_the_new_size() {
    let Some((_lock, mut renderer, mut runtime)) = attached("resize", TestImages::Kept(3)) else {
        return;
    };
    hold_frames(&mut renderer, &mut runtime);
    let first = renderer.last_published_frame_id() + 1;
    renderer.scene_mut().describe(&direct_graph());
    publish(&mut renderer);

    // Nothing pumps the inline runtime here: a resize that waited for the
    // present thread would not return.
    assert!(
        renderer
            .present_thread()
            .is_some_and(|thread| thread.resize(surface_config(WIDTH * 2, HEIGHT * 2))),
        "the resize reaches the present thread"
    );
    assert_eq!(
        renderer.publish_frame(WIDTH * 2, HEIGHT * 2),
        PublishOutcome::Published
    );
    runtime.pump();
    assert_eq!(
        drain_outcomes(&mut renderer),
        vec![
            (first, PresentOutcome::Cancelled(CancelReason::SurfaceEpoch)),
            (first + 1, PresentOutcome::Presented),
        ],
        "the frame built for the old size is refused and the next draws"
    );
}

#[test]
fn an_occluded_window_holds_the_newest_frame_and_draws_it_once_shown() {
    let Some((_lock, mut renderer, mut runtime)) = attached("occlusion", TestImages::Kept(3))
    else {
        return;
    };
    hold_frames(&mut renderer, &mut runtime);
    let first = renderer.last_published_frame_id() + 1;
    renderer.scene_mut().describe(&direct_graph());

    assert!(
        renderer
            .present_thread()
            .is_some_and(|thread| thread.set_occluded(true))
    );
    publish(&mut renderer);
    runtime.pump();
    assert!(
        drain_outcomes(&mut renderer).is_empty(),
        "an occluded window draws nothing"
    );
    renderer.scene_mut().describe(&shadowed_child_graph());
    publish(&mut renderer);
    runtime.pump();
    assert_eq!(
        drain_outcomes(&mut renderer),
        vec![(
            first,
            PresentOutcome::Cancelled(CancelReason::SurfaceUnavailable)
        )],
        "a newer frame stands in for the one held back"
    );

    assert!(
        renderer
            .present_thread()
            .is_some_and(|thread| thread.set_occluded(false))
    );
    runtime.pump();
    assert_eq!(
        drain_outcomes(&mut renderer),
        vec![(first + 1, PresentOutcome::Presented)],
        "the held frame draws once the window shows the surface"
    );
}

#[test]
fn a_new_device_takes_over_the_present_thread_and_refuses_older_frames() {
    let Some((_lock, mut renderer, mut runtime)) =
        attached("device replacement", TestImages::Kept(3))
    else {
        return;
    };
    let replacement = match gpu_target(wgpu::TextureFormat::Bgra8UnormSrgb) {
        Ok(target) => target,
        Err(err) => {
            eprintln!("skipping device replacement: no second device: {err}");
            return;
        }
    };
    hold_frames(&mut renderer, &mut runtime);
    let first = renderer.last_published_frame_id() + 1;
    renderer.scene_mut().describe(&direct_graph());
    publish(&mut renderer);

    replace_device_then_publish(&mut renderer, replacement);
    runtime.pump();
    assert_eq!(
        drain_outcomes(&mut renderer),
        vec![
            (
                first,
                PresentOutcome::Cancelled(CancelReason::RendererEpoch)
            ),
            (first + 1, PresentOutcome::Presented),
        ],
        "the frame built for the old device is refused and the next draws on the new one"
    );
}
