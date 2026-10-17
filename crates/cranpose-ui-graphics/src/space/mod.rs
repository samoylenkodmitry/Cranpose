//! Composables in 3D: meshes that a composable's content is drawn on, the
//! scene they are placed in and the camera that shows it.
//!
//! A layer whose graphics layer carries a [`LayerSpace::Scene`] shows a
//! scene in its bounds; each child whose graphics layer carries a
//! [`LayerSpace::Surface`] has its content drawn on a mesh in that scene,
//! where it still takes pointer input.

mod math;
mod mesh;
mod obj;
mod scene;
mod sdf;

pub use math::{Mat4, Vec3};
pub use mesh::{Mesh, MeshError, MeshHit};
pub use obj::{ObjError, ObjErrorKind};
pub use scene::{
    Camera, Glass, LayerSpace, Light, Material, MeshSurface, NEAR_PLANE, Ray, Scene3d, SurfaceHit,
    Volume,
};
