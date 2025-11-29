//! Common rendering contracts shared between renderer backends.

use compose_foundation::nodes::input::PointerEventKind;
use compose_ui::LayoutTree;
use compose_ui_graphics::Size;

pub use compose_ui_graphics::Brush;

/// Trait implemented by hit-test targets stored inside a [`RenderScene`].
pub trait HitTestTarget {
    fn dispatch(&self, kind: PointerEventKind, x: f32, y: f32);
}

/// Trait describing the minimal surface area required by the application
/// shell to process pointer events and refresh the frame graph.
pub trait RenderScene {
    type HitTarget: HitTestTarget;

    fn clear(&mut self);
    fn hit_test(&self, x: f32, y: f32) -> Vec<Self::HitTarget>;
}

/// Abstraction implemented by concrete renderer backends.
pub trait Renderer {
    type Scene: RenderScene;
    type Error;

    fn scene(&self) -> &Self::Scene;
    fn scene_mut(&mut self) -> &mut Self::Scene;

    fn rebuild_scene(
        &mut self,
        layout_tree: &LayoutTree,
        viewport: Size,
    ) -> Result<(), Self::Error>;
}
