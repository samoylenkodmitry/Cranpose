//! The layers that 3D content adds to a headset frame, and where every layer stands in the list.
//!
//! The compositor draws layers in list order, and the last one covers the others. The room is
//! the bottom layer and the rays are the top. The 3D content is one projection layer between
//! the streamed scenes and the panels, so every panel stands over it in the list.
//!
//! A frame with more layers than the runtime takes draws the pictures of some panels and handles
//! into the projection layer too, so they stand under every layer of the panels in the list.
//!
//! A runtime that offers `XR_KHR_composition_layer_depth` and `XR_FB_composition_layer_depth_test`
//! tests each layer over the projection layer against its depth. A panel behind a 3D object then
//! shows behind it, and a panel in front shows in front. Without the test, a panel covers the
//! 3D content wherever it stands.

use crate::{
    METERS_PER_DP, Pose,
    space::{PanelKind, PanelSlot, Ride},
    vector::{add, dot, scale, sub},
};

// The tail of the frame's list of layers, from the bottom to the top. The room's layer comes
// before it, outside this list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stage {
    // Projection layers of frames that another machine rendered
    Streams,
    // The one projection layer of the app's 3D content
    Volumes,
    // Flat and curved panels, surfaces and the handles of panels
    Panels,
    // The controllers' rays
    Overlay,
}

// The stages in the order the compositor draws them.
pub(crate) const STAGES: [Stage; 4] = [
    Stage::Streams,
    Stage::Volumes,
    Stage::Panels,
    Stage::Overlay,
];

// What the runtime and the session give for depth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DepthSupport {
    // Whether the layer can carry depth: the instance has `XR_KHR_composition_layer_depth` and the
    // runtime offers a depth swapchain format that the scene draws against
    pub(crate) submits: bool,
    // Whether the instance has `XR_FB_composition_layer_depth_test`
    pub(crate) tests: bool,
}

// What a frame does for its 3D content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VolumePlan {
    // Whether each eye's view carries its depth
    pub(crate) submits_depth: bool,
    // Whether the layers over the projection layer test against its depth
    pub(crate) tests_depth: bool,
}

// The plan for a frame with `contents` things that the projection layer draws: the volumes and
// controller models of the app's composition and the pictures of layers that the frame draws in
// place of their own. `None` when the frame has no projection layer: it draws nothing and costs
// the GPU nothing. The layer needs a view of each eye, which `eyes` says the runtime located.
pub(crate) fn plan(contents: usize, eyes: [bool; 2], depth: DepthSupport) -> Option<VolumePlan> {
    (contents > 0 && eyes == [true, true]).then_some(VolumePlan {
        submits_depth: depth.submits,
        // The test reads the depth that the layer carries
        tests_depth: depth.submits && depth.tests,
    })
}

// Whether the layers of `stage` test against the depth of the projection layer in a frame with
// `plan`. The room and the streamed scenes lie under the projection layer.
pub(crate) fn tests_depth(stage: Stage, plan: Option<VolumePlan>) -> bool {
    matches!(stage, Stage::Panels | Stage::Overlay) && plan.is_some_and(|plan| plan.tests_depth)
}

// Whether the layers of `stage` write their depth into the compositor's depth buffer in a frame
// with `plan`. The buffer starts at the far plane, and a layer writes into it only when it asks
// for the depth test with the write on, so the projection layer asks for it. A layer with no
// depth test leaves the buffer as it was, and the panels over it would have nothing to test.
pub(crate) fn writes_depth(stage: Stage, plan: Option<VolumePlan>) -> bool {
    stage == Stage::Volumes && plan.is_some_and(|plan| plan.tests_depth)
}

// Whether the layer of the panel `index` of `panels` skips the compositor's depth test, as `head`
// sees the panels. A panel that carries the words of a tray skips it: the tray's parts lie a
// millimeter or more behind the words, and a Quest 3's compositor cuts the words with its test
// where the tray stands off the view's center, though the depth of the projection layer is exact
// there. The words keep the test while another volume stands in front of them, so that the volume
// covers them.
pub(crate) fn skips_depth_test(panels: &[PanelSlot], index: usize, head: Option<[f32; 3]>) -> bool {
    let panel = &panels[index];
    let own = |slot: &PanelSlot| matches!(slot.kind, PanelKind::Volume { under: Some(id), .. } if id == panel.id);
    if !panels.iter().any(own) {
        return false;
    }
    let Some(head) = head else {
        return true;
    };
    !panels.iter().any(|slot| {
        matches!(slot.kind, PanelKind::Volume { .. })
            && !own(slot)
            && slot.ride != Ride::Hidden
            && covers(slot, panel, head)
    })
}

// Whether the front face of the volume `near` covers a part of the panel `far` as `head` sees
// them. The corners of the face that lie between the head and the panel's plane, carried along the
// lines of sight onto that plane, span a rectangle that meets the panel's.
fn covers(near: &PanelSlot, far: &PanelSlot, head: [f32; 3]) -> bool {
    let face = near.face();
    let [width, height] = near.size().map(|dp| dp * METERS_PER_DP / 2.0);
    let [far_width, far_height] = far.size().map(|dp| dp * METERS_PER_DP / 2.0);
    let normal = far.pose.rotate([0.0, 0.0, 1.0]);
    let plane = dot(sub(far.pose.position, head), normal);
    let mut span: Option<[f32; 4]> = None;
    for [x, y] in [
        [-width, -height],
        [width, -height],
        [-width, height],
        [width, height],
    ] {
        let sight = sub(face.then(&Pose::at([x, y, 0.0])).position, head);
        let along = dot(sight, normal);
        // The line of sight meets the plane past the corner only when the corner is in front
        if along.abs() <= f32::EPSILON || plane / along <= 1.0 {
            continue;
        }
        let met = scale(sight, plane / along);
        let [x, y, _] = far.pose.unrotate(sub(add(head, met), far.pose.position));
        span = Some(span.map_or([x, x, y, y], |[left, right, bottom, top]| {
            [left.min(x), right.max(x), bottom.min(y), top.max(y)]
        }));
    }
    span.is_some_and(|[left, right, bottom, top]| {
        left < far_width && right > -far_width && bottom < far_height && top > -far_height
    })
}

#[cfg(test)]
#[path = "tests/volume_plan_tests.rs"]
mod tests;
