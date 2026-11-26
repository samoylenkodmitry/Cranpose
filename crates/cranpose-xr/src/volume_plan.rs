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

impl DepthSupport {
    // What a frame can do with the depth of its projection layer, whose content `leaves_depth` or
    // not. A layer whose content leaves none, such as holograms alone, carries no depth. The frame
    // then skips the depth's pass and its store. The compositor tests no layer against it.
    pub(crate) fn for_content(self, leaves_depth: bool) -> Self {
        Self {
            submits: self.submits && leaves_depth,
            ..self
        }
    }
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

#[cfg(test)]
#[path = "tests/volume_plan_tests.rs"]
mod tests;
