// What every room of the backrooms shares, which the tab provides to all of
// them through `local_level`: the meshes their surfaces are drawn on, the
// journey, the camcorder's film and the shelf of printed source files. The
// window's eye reaches them through the framework's `local_eye`.

use std::sync::Arc;

use cranpose_core::{staticCompositionLocalOf, State, StaticCompositionLocal};
use cranpose_ui::space::Mesh;

use super::{journey::Journey, shelf::Shelf};

// The meshes every room's surfaces are drawn on, made once.
#[derive(Clone)]
pub(super) struct Meshes {
    pub(super) plane: Arc<Mesh>,
    pub(super) sphere: Arc<Mesh>,
    pub(super) cuboid: Arc<Mesh>,
}

impl Meshes {
    pub(super) fn new() -> Self {
        Self {
            plane: Arc::new(Mesh::plane()),
            sphere: Arc::new(Mesh::sphere(32, 16)),
            cuboid: Arc::new(Mesh::cuboid()),
        }
    }
}

impl PartialEq for Meshes {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.plane, &other.plane)
            && Arc::ptr_eq(&self.sphere, &other.sphere)
            && Arc::ptr_eq(&self.cuboid, &other.cuboid)
    }
}

#[derive(Clone, PartialEq)]
pub(super) struct Level {
    pub(super) meshes: Meshes,
    pub(super) journey: Journey,
    // The camcorder's frame, a whole number 24 times a second, by which the
    // grain moves and the lamps flicker. Draw closures read it: a new frame
    // draws the rooms again and composes nothing.
    pub(super) film: State<f32>,
    pub(super) shelf: Shelf,
}

// The level around a composable of the backrooms; `None` outside the tab.
// The tab provides it once and never changes it, so it is static: reading
// it subscribes nothing.
pub(super) fn local_level() -> StaticCompositionLocal<Option<Level>> {
    thread_local! {
        static LEVEL: StaticCompositionLocal<Option<Level>> = staticCompositionLocalOf(|| None);
    }
    LEVEL.with(Clone::clone)
}
