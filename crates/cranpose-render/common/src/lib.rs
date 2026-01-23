//! Common rendering contracts shared between renderer backends.

use cranpose_foundation::nodes::input::PointerEvent;
use cranpose_ui::LayoutTree;
use cranpose_ui_graphics::Size;

pub use cranpose_ui_graphics::Brush;

/// Trait implemented by hit-test targets stored inside a [`RenderScene`].
pub trait HitTestTarget {
    /// Dispatches a pointer event to this target's handlers.
    fn dispatch(&self, event: PointerEvent);

    /// Returns the NodeId associated with this hit target.
    /// Used by HitPathTracker to cache stable identity instead of geometry.
    fn node_id(&self) -> cranpose_core::NodeId;
}

/// Trait describing the minimal surface area required by the application
/// shell to process pointer events and refresh the frame graph.
pub trait RenderScene {
    type HitTarget: HitTestTarget + Clone;

    fn clear(&mut self);

    /// Performs hit testing at the given coordinates.
    /// Returns hit targets ordered by z-index (top-to-bottom).
    fn hit_test(&self, x: f32, y: f32) -> Vec<Self::HitTarget>;

    /// Returns NodeIds of all hit regions at the given coordinates.
    /// This is a convenience method equivalent to `hit_test().map(|h| h.node_id())`.
    fn hit_test_nodes(&self, x: f32, y: f32) -> Vec<cranpose_core::NodeId> {
        self.hit_test(x, y)
            .into_iter()
            .map(|h| h.node_id())
            .collect()
    }

    /// Finds a hit target by NodeId with fresh geometry from the current scene.
    ///
    /// This is the key method for HitPathTracker-style gesture handling:
    /// - On PointerDown, we cache NodeIds (not geometry)
    /// - On Move/Up/Cancel, we call this to get fresh HitTarget with current geometry
    /// - Handler closures are preserved (same Rc), so internal state survives
    ///
    /// Returns None if the node no longer exists in the scene (e.g., removed during gesture).
    fn find_target(&self, node_id: cranpose_core::NodeId) -> Option<Self::HitTarget>;
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

    /// Rebuilds the scene by traversing the LayoutNode tree directly via Applier.
    ///
    /// This is the new architecture that eliminates per-frame LayoutTree reconstruction.
    /// Implementors should read layout state from LayoutNode.layout_state() directly.
    ///
    /// Default implementation panics - implementors must upgrade to support this.
    fn rebuild_scene_from_applier(
        &mut self,
        _applier: &mut cranpose_core::MemoryApplier,
        _root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        panic!("rebuild_scene_from_applier not implemented - renderer must be upgraded for direct LayoutNode rendering")
    }

    /// Draw a development overlay (e.g., FPS counter) on top of the scene.
    ///
    /// This is called after rebuild_scene when dev options are enabled.
    /// The text is drawn directly by the renderer without affecting composition.
    ///
    /// Default implementation does nothing.
    fn draw_dev_overlay(&mut self, _text: &str, _viewport: Size) {
        // Default: no-op
    }
}
