//! The layers of the frame being ended, as the raw structs that the runtime reads.
//!
//! The list keeps its capacity from frame to frame. A raw layer names its swapchain by handle, so
//! the list holds no borrow. `fill` takes the borrowed layers and ends their use when it returns.

use std::ptr;

use openxr as xr;

use super::{
    cylinder_layers::CylinderLayer,
    graphics::Api,
    session::{ProjectionLayer, QuadLayer},
};

// A layer of a panel: flat or curved.
pub(crate) enum Layer<'a> {
    Quad(QuadLayer<'a>),
    Cylinder(CylinderLayer<'a>),
}

enum RawLayer {
    // The views are the layer's own, so the struct's pointer to them is set when the frame ends.
    Projection {
        layer: xr::sys::CompositionLayerProjection,
        views: [xr::sys::CompositionLayerProjectionView; 2],
    },
    Quad(xr::sys::CompositionLayerQuad),
    Cylinder(xr::sys::CompositionLayerCylinderKHR),
}

pub(crate) struct FrameLayers {
    space: xr::sys::Space,
    layers: Vec<RawLayer>,
    headers: Vec<*const xr::sys::CompositionLayerBaseHeader>,
}

impl FrameLayers {
    // An empty list of layers in `space`, which must outlive the list's use.
    pub(crate) fn new(space: xr::sys::Space) -> Self {
        Self {
            space,
            layers: Vec::new(),
            headers: Vec::new(),
        }
    }

    // Replaces the layers with `streams`, then `panels`, then `overlay`, in the order that the
    // compositor draws them.
    pub(crate) fn fill<'a>(
        &mut self,
        streams: impl Iterator<Item = ProjectionLayer<'a>>,
        panels: impl Iterator<Item = Layer<'a>>,
        overlay: impl Iterator<Item = QuadLayer<'a>>,
    ) {
        self.clear();
        let space = self.space;
        self.layers.extend(streams.map(|stream| {
            let (layer, views) = stream.to_raw(space);
            RawLayer::Projection { layer, views }
        }));
        self.layers.extend(panels.map(|panel| match panel {
            Layer::Quad(quad) => RawLayer::Quad(quad.to_raw(space)),
            Layer::Cylinder(cylinder) => RawLayer::Cylinder(cylinder.to_raw(space)),
        }));
        self.layers
            .extend(overlay.map(|quad| RawLayer::Quad(quad.to_raw(space))));
    }

    // The pointers to the layers' structs, with `bottom` first, for the call that ends the frame.
    // `flip`, if any, is the layout that every flat and curved layer says its image has. The
    // pointers go stale when the list changes.
    pub(crate) fn headers(
        &mut self,
        bottom: Option<&xr::CompositionLayerBase<'_, Api>>,
        flip: Option<&xr::sys::CompositionLayerImageLayoutFB>,
    ) -> &[*const xr::sys::CompositionLayerBaseHeader] {
        let chain = flip.map_or(ptr::null(), |flip| ptr::from_ref(flip).cast());
        self.headers.clear();
        self.headers
            .extend(bottom.map(|layer| ptr::from_ref(layer).cast()));
        for layer in &mut self.layers {
            let header = match layer {
                RawLayer::Projection { layer, views } => {
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
    }
}
