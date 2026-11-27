//! The 3D space that one layer gives its node, built once and moved in place.
//!
//! A node's graphics layer holds its space in an `Arc`. The node keeps the space of the last
//! evaluation until the next one replaces it. A closure that builds a fresh space allocates the
//! `Arc` and the list of the layer's surfaces at each evaluation, so a model that moves allocates in
//! each frame. This ring keeps a few spaces. An evaluation picks one that no node holds any more
//! and moves its surfaces in place.

use std::{cell::RefCell, sync::Arc};

use cranpose::space::{LayerSpace, Mat4, Material, Mesh, MeshSurface, MeshSurfaces};

// The spaces that a layer keeps. The node holds the space of the last evaluation and the evaluation
// in progress takes another, so two always serve. The third is for a space that a frame in flight
// still holds.
const SPACES: usize = 3;

pub(crate) struct SurfaceRing {
    spaces: RefCell<Vec<Arc<LayerSpace>>>,
}

impl SurfaceRing {
    pub(crate) fn new() -> Self {
        Self {
            spaces: RefCell::new(Vec::with_capacity(SPACES)),
        }
    }

    // The space of a layer whose surfaces show `meshes`, the first as the owner, with the surface
    // `index` placed by `place(index)`. `meshes` is not empty. A call allocates only while the
    // ring fills and where every kept space is still held.
    pub(crate) fn space(
        &self,
        meshes: &[Arc<Mesh>],
        place: impl Fn(usize) -> Mat4,
    ) -> Arc<LayerSpace> {
        let mut reused = {
            let mut spaces = self.spaces.borrow_mut();
            spaces
                .iter()
                .position(|kept| Arc::strong_count(kept) == 1)
                .map(|index| spaces.swap_remove(index))
        };
        if let Some(mut kept) = reused.take()
            && let Some(LayerSpace::Surface(surfaces)) = Arc::get_mut(&mut kept)
        {
            for (index, surface) in surfaces.iter_mut().enumerate() {
                surface.transform = place(index);
            }
            let mut spaces = self.spaces.borrow_mut();
            if spaces.len() < SPACES {
                spaces.push(Arc::clone(&kept));
            }
            return kept;
        }
        let surface = |index: usize| MeshSurface {
            mesh: Arc::clone(&meshes[index]),
            transform: place(index),
            material: Material::Lit,
            casts_shadow: false,
            solid: true,
        };
        let built = Arc::new(LayerSpace::Surface(MeshSurfaces::new(
            surface(0),
            (1..meshes.len()).map(surface).collect(),
        )));
        let mut spaces = self.spaces.borrow_mut();
        if spaces.len() < SPACES {
            spaces.push(Arc::clone(&built));
        }
        built
    }
}

#[cfg(test)]
#[path = "tests/surface_ring_tests.rs"]
mod tests;
