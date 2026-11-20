//! The 3D space that one layer of a model gives its node, built once and moved in place.
//!
//! A node's graphics layer holds its space in an `Arc`, and the node keeps the space of the last
//! evaluation until the next one replaces it. A closure that builds a fresh space allocates the
//! `Arc` and the list of the layer's surfaces at each evaluation, so an animation allocates in each
//! frame. This ring keeps a few spaces. An evaluation picks one that no node holds any more and
//! moves its surfaces in place.

use std::{cell::RefCell, sync::Arc};

use cranpose::space::{LayerSpace, Mat4, Material, MeshSurface, MeshSurfaces};

use crate::{gltf_model::GltfModel, spatial_gltf_model_state::LayerDraws};

// The spaces that a layer keeps. The node holds the space of the last evaluation and the evaluation
// in progress takes another, so two always serve. The third is for a space that a frame in flight
// still holds.
const SPACES: usize = 3;

pub(crate) struct LayerSpaces {
    spaces: RefCell<Vec<Arc<LayerSpace>>>,
}

impl LayerSpaces {
    pub(crate) fn new() -> Self {
        Self {
            spaces: RefCell::new(Vec::with_capacity(SPACES)),
        }
    }

    // The space of the layer `draws` covers, with each surface placed by `place` from the draw's
    // node. A call allocates only while the ring fills and where every kept space is still held.
    pub(crate) fn space(
        &self,
        (model, draws): (&GltfModel, &LayerDraws),
        place: impl Fn(usize) -> Mat4,
    ) -> Arc<LayerSpace> {
        let mut spaces = self.spaces.borrow_mut();
        for kept in spaces.iter_mut() {
            if let Some(LayerSpace::Surface(surfaces)) = Arc::get_mut(kept) {
                let placed = std::iter::once(&draws.owner).chain(&draws.more);
                for (surface, &draw) in surfaces.iter_mut().zip(placed) {
                    surface.transform = place(model.draws[draw].node);
                }
                return Arc::clone(kept);
            }
        }
        let surface = |draw: usize| {
            let draw = &model.draws[draw];
            MeshSurface {
                mesh: Arc::clone(&draw.mesh),
                transform: place(draw.node),
                material: Material::Lit,
                casts_shadow: false,
                solid: true,
            }
        };
        let built = Arc::new(LayerSpace::Surface(MeshSurfaces::new(
            surface(draws.owner),
            draws.more.iter().map(|&draw| surface(draw)).collect(),
        )));
        if spaces.len() < SPACES {
            spaces.push(Arc::clone(&built));
        }
        built
    }
}

#[cfg(test)]
#[path = "tests/gltf_layer_spaces_tests.rs"]
mod tests;
