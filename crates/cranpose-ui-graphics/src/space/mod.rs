//! Composables in 3D: meshes that a composable's content is drawn on, the
//! scene they are placed in and the camera that shows it.
//!
//! A layer whose graphics layer carries a [`LayerSpace::Scene`] shows a
//! scene in its bounds. Every layer inside it takes a 3D frame from its
//! parent's, through its place in layout; a [`LayerSpace::Surface`] draws
//! the layer's content on a mesh in its frame, where it still takes pointer
//! input, and a [`LayerSpace::Transform`] moves the frames beneath it. A
//! scene inside a scene is a doorway into a space of its own, which the
//! window's eye sees through and walks into (see [`EyePose`] and
//! [`EyeWorld`]).

mod eye;
mod math;
mod mesh;
mod obj;
mod scene;
mod sdf;

pub use eye::{EyePlace, EyePose, EyeWorld, PITCH_LIMIT, WorldSink};
pub use math::{Mat4, Vec3};
pub use mesh::{Mesh, MeshContact, MeshError, MeshHit};
pub use obj::{ObjError, ObjErrorKind};
pub use scene::{
    Camera, Glass, LayerSpace, Light, Material, MeshSurface, NEAR_PLANE, Ray, Scene3d, Shaded,
    SurfaceHit, Volume,
};
