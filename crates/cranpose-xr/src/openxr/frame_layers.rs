//! The layers of the frame being ended, as the raw structs that the runtime reads.
//!
//! The list keeps its capacity from frame to frame. A raw layer names its swapchain by handle, so
//! the list holds no borrow. `fill` takes the borrowed layers and ends their use when it returns.

use std::{ffi::c_void, ptr};

use openxr as xr;

use super::{
    cylinder_layers::CylinderLayer,
    graphics::Api,
    session::{ProjectionLayer, QuadLayer, RawProjection},
};
use crate::volume_plan::{STAGES, Stage, VolumePlan, tests_depth};

// A layer of a panel: flat or curved.
pub(crate) enum Layer<'a> {
    Quad(QuadLayer<'a>),
    Cylinder(CylinderLayer<'a>),
}

// The layers of a frame in the stages that the compositor draws, from the bottom to the top. The
// room's layer is under all of them. The order of the stages is `STAGES`.
pub(crate) struct Stages<'a, Streams, Panels, Overlay> {
    pub(crate) streams: Streams,
    // The one projection layer of the app's 3D content, if the frame has it
    pub(crate) volumes: Option<ProjectionLayer<'a>>,
    pub(crate) panels: Panels,
    pub(crate) overlay: Overlay,
}

enum RawLayer {
    // The index in the frame's projection layers, which hold the structs of the layer's views, so
    // that the pointers to them are set when the frame ends.
    Projection(usize),
    Quad(xr::sys::CompositionLayerQuad),
    Cylinder(xr::sys::CompositionLayerCylinderKHR),
}

struct Raw {
    layer: RawLayer,
    // Whether the layer tests against the depth of the projection layer that the frame carries
    depth_tested: bool,
}

// The structs that the `next` pointer of a quad or a cylinder names, by what the layer is.
#[derive(Clone, Copy)]
pub(crate) struct Chains {
    // For a layer that the frame draws without a depth test
    pub(crate) plain: *const c_void,
    // For a layer that tests against the depth of the projection layer
    pub(crate) tested: *const c_void,
}

pub(crate) struct FrameLayers {
    space: xr::sys::Space,
    layers: Vec<Raw>,
    projections: Vec<RawProjection>,
    headers: Vec<*const xr::sys::CompositionLayerBaseHeader>,
}

impl FrameLayers {
    // An empty list of layers in `space`, which must outlive the list's use.
    pub(crate) fn new(space: xr::sys::Space) -> Self {
        Self {
            space,
            layers: Vec::new(),
            projections: Vec::new(),
            headers: Vec::new(),
        }
    }

    // Replaces the layers with those of `stages`, in the order that the compositor draws them.
    // `plan` says which stages test depth.
    pub(crate) fn fill<'a>(
        &mut self,
        stages: Stages<
            'a,
            impl Iterator<Item = ProjectionLayer<'a>>,
            impl Iterator<Item = Layer<'a>>,
            impl Iterator<Item = QuadLayer<'a>>,
        >,
        plan: Option<VolumePlan>,
    ) {
        self.clear();
        let space = self.space;
        let Stages {
            streams,
            volumes,
            panels,
            overlay,
        } = stages;
        let (mut streams, mut panels, mut overlay) = (Some(streams), Some(panels), Some(overlay));
        for stage in STAGES {
            let depth_tested = tests_depth(stage, plan);
            let raw = |layer| Raw {
                layer,
                depth_tested,
            };
            match stage {
                Stage::Streams => {
                    for stream in streams.take().into_iter().flatten() {
                        self.projections.push(stream.to_raw(space));
                        let layer = RawLayer::Projection(self.projections.len() - 1);
                        self.layers.push(raw(layer));
                    }
                }
                Stage::Volumes => {
                    if let Some(volume) = &volumes {
                        self.projections.push(volume.to_raw(space));
                        let layer = RawLayer::Projection(self.projections.len() - 1);
                        self.layers.push(raw(layer));
                    }
                }
                Stage::Panels => {
                    self.layers
                        .extend(panels.take().into_iter().flatten().map(|panel| {
                            raw(match panel {
                                Layer::Quad(quad) => RawLayer::Quad(quad.to_raw(space)),
                                Layer::Cylinder(cylinder) => {
                                    RawLayer::Cylinder(cylinder.to_raw(space))
                                }
                            })
                        }));
                }
                Stage::Overlay => {
                    self.layers.extend(
                        overlay
                            .take()
                            .into_iter()
                            .flatten()
                            .map(|quad| raw(RawLayer::Quad(quad.to_raw(space)))),
                    );
                }
            }
        }
    }

    // The pointers to the layers' structs, with `bottom` first, for the call that ends the frame.
    // `chains` are the structs that every flat and curved layer names: the layout that says its
    // image starts with the top row and the depth test. The pointers go stale when the list
    // changes.
    pub(crate) fn headers(
        &mut self,
        bottom: Option<&xr::CompositionLayerBase<'_, Api>>,
        chains: Chains,
    ) -> &[*const xr::sys::CompositionLayerBaseHeader] {
        self.headers.clear();
        self.headers
            .extend(bottom.map(|layer| ptr::from_ref(layer).cast()));
        for Raw {
            layer,
            depth_tested,
        } in &mut self.layers
        {
            let chain = if *depth_tested {
                chains.tested
            } else {
                chains.plain
            };
            let header = match layer {
                RawLayer::Projection(index) => {
                    let RawProjection {
                        layer,
                        views,
                        depths,
                    } = &mut self.projections[*index];
                    for (view, depth) in views.iter_mut().zip(depths.iter()) {
                        view.next = depth
                            .as_ref()
                            .map_or(ptr::null(), |depth| ptr::from_ref(depth).cast());
                    }
                    layer.views = views.as_ptr();
                    ptr::from_ref(&*layer).cast()
                }
                RawLayer::Quad(quad) => {
                    quad.next = chain;
                    ptr::from_ref(&*quad).cast()
                }
                RawLayer::Cylinder(cylinder) => {
                    cylinder.next = chain;
                    ptr::from_ref(&*cylinder).cast()
                }
            };
            self.headers.push(header);
        }
        &self.headers
    }

    // Empties the list. The raw layers name swapchains. The pointers name the list's own
    // structs. None of them outlives the frame.
    pub(crate) fn clear(&mut self) {
        self.headers.clear();
        self.layers.clear();
        self.projections.clear();
    }
}
