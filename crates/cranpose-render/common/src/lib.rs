//! Common rendering contracts shared between renderer backends.

#![deny(unsafe_code)]

pub mod bounded_lru_cache;
pub mod brush_sampling;
pub mod font_layout;
pub mod geometry;
pub mod graph;
mod graph_hash;
pub mod graph_scene;
pub mod hit_graph;
pub mod image_compare;
pub mod layer_composition;
pub mod layer_shadow;
pub mod layer_transform;
pub mod primitive_emit;
pub mod raster_cache;
pub mod render_contract;
pub mod scene_builder;
pub mod software_text_raster;
pub mod style_shared;
pub mod text_hyphenation;

use cranpose_core::MemoryApplier;
use cranpose_foundation::nodes::input::PointerEvent;
use cranpose_ui::LayoutTree;
use cranpose_ui_graphics::Size;

pub use cranpose_ui_graphics::Brush;

/// Trait implemented by hit-test targets stored inside a [`RenderScene`].
pub trait HitTestTarget {
    /// Dispatches a pointer event to this target's handlers.
    fn dispatch(&self, event: PointerEvent);

    /// Dispatches a pointer event using the current live node state when available.
    ///
    /// Render-scene hit targets may cache closures from an older scene build. Pointer
    /// dispatch goes through this hook so implementations can resolve fresh handlers
    /// from the current applier while still using the target's geometry snapshot.
    fn dispatch_with_applier(&self, _applier: &mut MemoryApplier, event: PointerEvent) {
        self.dispatch(event);
    }

    /// Returns the NodeId associated with this hit target.
    /// Used by HitPathTracker to cache stable identity instead of geometry.
    fn node_id(&self) -> cranpose_core::NodeId;

    /// Returns the node capture path that should stay attached to this target's gesture.
    ///
    /// The default is just this target's own node. Renderers can override this to
    /// include stable ancestor pointer-input nodes that must continue receiving
    /// Move/Up/Cancel even if the original descendant target is recycled.
    fn capture_path(&self) -> Vec<cranpose_core::NodeId> {
        vec![self.node_id()]
    }
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

    /// Installs renderer-provided app services into the target AppContext.
    ///
    /// AppShell calls this before the first composition pass.
    /// Renderers that provide text measurement or other per-app services should install
    /// them here rather than as constructor side effects.
    fn attach_app_context_services(&mut self, _app_context: &cranpose_ui::AppContext) {}

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
    /// Implementors must read layout state from LayoutNode.layout_state() directly.
    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        viewport: Size,
    ) -> Result<(), Self::Error>;

    fn update_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        viewport: Size,
        dirty_nodes: &[cranpose_core::NodeId],
    ) -> Result<(), Self::Error> {
        let _ = dirty_nodes;
        self.rebuild_scene_from_applier(applier, root, viewport)
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
