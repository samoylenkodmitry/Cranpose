#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod annotated_text;
mod ascii_glyphs;
/// Debug switches read from environment variables, with programmatic overrides.
pub mod debug_toggles;
pub mod dev_overlay;
mod direct_mapped_cache;

/// The frame background every renderer clears to (linear values; sRGB
/// surfaces display this as rgb(75, 75, 86)). One definition so backends
/// cannot drift.
pub const FRAME_CLEAR_COLOR: [f32; 4] = [18.0 / 255.0, 18.0 / 255.0, 24.0 / 255.0, 1.0];
/// CPU sampling of brushes at a point, for the software renderers.
pub mod brush_sampling;
mod font_features;
/// Glyph layout and vertical metrics of a font at one pixel size.
pub mod font_layout;
pub mod font_source;
mod font_tracking;
mod full_glyph_count;
/// Blur kernel and rectangle helpers shared by the renderers.
pub mod geometry;
pub mod gpos_kerning;
/// The render graph: the layers and primitives that a scene builds for a renderer to draw.
pub mod graph;
mod graph_hash;
/// The retained scene: the render graph together with the hit targets built from it.
pub mod graph_scene;
/// Collecting hit targets from a render graph.
pub mod hit_graph;
/// Pixel comparison and resampling of RGBA images, for renderer tests.
pub mod image_compare;
#[cfg(feature = "installed-fonts")]
pub mod installed_fonts;
/// Rules for when a graphics layer is drawn into its own offscreen buffer and with which alpha.
pub mod layer_composition;
mod layer_recycling;
pub mod layer_shadow;
/// Transform math of a graphics layer, re-exported from `cranpose_ui_graphics`.
pub mod layer_transform;
/// Lowering of draw primitives to the shape, image and text draw calls of a renderer.
pub mod primitive_emit;
/// Keys of the cached rasters of layers.
pub mod raster_cache;
/// Shared cases that every renderer backend is checked against.
pub mod render_contract;
/// Building and updating the render graph from the layout tree.
pub mod scene_builder;
/// Which mesh surface of a 3D scene a window point shows.
pub mod scene_pick;
pub mod shape_sdf;
/// CPU text rasterization: fonts, glyph layout and glyph masks.
pub mod software_text_raster;
/// Where a window's scenes put their 3D layers: frames, spaces, doorways
/// and what the eye sees.
pub mod space;
/// Colors, brushes and corner radii resolved against the properties of a graphics layer.
pub mod style_shared;
mod text_cache_key;
/// Automatic hyphenation of text, using dictionaries that the app registers.
pub mod text_hyphenation;
pub mod text_mask_gamma;
pub mod text_measure;
pub mod text_paint;
#[cfg(feature = "text-shaping")]
mod text_shaping;
mod thread_state;

#[cfg(test)]
#[path = "tests/pointer_slices.rs"]
mod pointer_slices;

use cranpose_core::{MemoryApplier, collections::map::HashSet};
use cranpose_foundation::nodes::input::PointerEvent;
use cranpose_ui::LayoutTree;
pub use cranpose_ui_graphics::Brush;
use cranpose_ui_graphics::Size;

/// Describes scene nodes whose recorded content or layer properties changed.
///
/// Content updates may re-record a node's draw commands. Layer updates change
/// graphics-layer properties while retaining those recordings. If a node
/// appears in both slices, content dirt takes precedence.
/// Layout, child structure, and draw-command changes belong in `content`.
///
/// ```
/// use cranpose_render_common::SceneUpdates;
/// let updates = SceneUpdates {
///     content: &[1],
///     layers: &[2],
///     moved: &[3],
/// };
/// assert!(!updates.is_empty());
/// ```
#[derive(Clone, Copy, Debug, Default)]
pub struct SceneUpdates<'a> {
    /// Nodes whose recorded draw content must be refreshed.
    pub content: &'a [cranpose_core::NodeId],
    /// Nodes whose graphics-layer properties changed without content dirt.
    pub layers: &'a [cranpose_core::NodeId],
    /// Nodes that moved in their parent and kept their size: the scene moves
    /// what it drew for them unless another list names them too.
    pub moved: &'a [cranpose_core::NodeId],
}

impl<'a> SceneUpdates<'a> {
    /// Creates an update containing only nodes with changed draw content.
    pub const fn content(nodes: &'a [cranpose_core::NodeId]) -> Self {
        Self {
            content: nodes,
            layers: &[],
            moved: &[],
        }
    }

    /// Returns whether the update contains no dirty nodes.
    pub const fn is_empty(self) -> bool {
        self.content.is_empty() && self.layers.is_empty() && self.moved.is_empty()
    }
}

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

    /// The pointer's appearance while it hovers this target, when the target
    /// names one.
    ///
    /// The shell asks the hit list top-down and applies the first answer, so
    /// the innermost region under the pointer decides the cursor.
    fn pointer_icon(&self) -> Option<cranpose_ui_graphics::PointerIcon> {
        None
    }

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
    /// The hit target type that hit testing returns.
    type HitTarget: HitTestTarget + Clone;

    /// Removes the render graph and all hit targets.
    fn clear(&mut self);

    /// Performs hit testing at the given coordinates.
    /// Returns hit targets ordered by z-index (top-to-bottom).
    fn hit_test(&self, x: f32, y: f32) -> Vec<Self::HitTarget>;

    /// The one target a press reaches when it misses every target but lands
    /// inside a small target grown to the minimum touch size: the nearest such
    /// target, and none when the point is outside every grown target.
    fn hit_test_near(&self, _x: f32, _y: f32) -> Option<Self::HitTarget> {
        None
    }

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

    /// The hit targets on the mesh surfaces of the scene the node `scene`
    /// shows that `ray` reaches, in the space the scene's eye stands in,
    /// ordered by z-index (top-to-bottom): a controller's, a hand's or a
    /// gaze's, which need not start at the eye.
    fn hit_test_ray(
        &self,
        _scene: cranpose_core::NodeId,
        _ray: cranpose_ui_graphics::space::Ray,
    ) -> Vec<Self::HitTarget> {
        Vec::new()
    }

    /// Visits each mesh surface the eyes of the window's scenes see, by its
    /// node, with the map from its content to the window as the eye shows
    /// it, through the doorways it lies behind: where the nodes on it show.
    fn surface_windows(
        &self,
        _visit: &mut dyn FnMut(cranpose_core::NodeId, cranpose_ui_graphics::ProjectiveTransform),
    ) {
    }

    /// Where the hit target of `node_id` shows in the window when it lies on
    /// a mesh surface of a 3D scene: its bounds as the scene's camera shows
    /// them, also through a doorway and for an eye that walked through one.
    /// `None` for a flat target, a target behind the eye or no target.
    fn surface_window_bounds(
        &self,
        _node_id: cranpose_core::NodeId,
    ) -> Option<cranpose_ui_graphics::Rect> {
        None
    }

    /// Replaces the set with retained visual observation owners, preserving its capacity.
    /// Returns whether the scene provides this information.
    fn collect_retained_visual_observation_nodes(
        &self,
        nodes: &mut HashSet<cranpose_core::NodeId>,
    ) -> bool {
        nodes.clear();
        false
    }
}

/// Abstraction implemented by concrete renderer backends.
pub trait Renderer {
    /// The scene type that holds the render graph and the hit targets of this renderer.
    type Scene: RenderScene;
    /// Error returned when the scene cannot be rebuilt or updated.
    type Error;

    /// Installs renderer-provided app services into the target AppContext.
    ///
    /// AppShell calls this before the first composition pass.
    /// Renderers that provide text measurement or other per-app services should install
    /// them here rather than as constructor side effects.
    fn attach_app_context_services(&mut self, _app_context: &cranpose_ui::AppContext) {}

    /// Takes in the fonts installed since the renderer last looked (see
    /// `installed_fonts::install_font_file`). A renderer that installed the
    /// app's text measurer installs one with the new faces, so text lays out
    /// again with them.
    ///
    /// AppShell calls this before the layout of a frame that follows an
    /// install.
    fn apply_installed_fonts(&mut self, _app_context: &cranpose_ui::AppContext) {}

    /// Sets how many device pixels the renderer draws for one logical unit.
    ///
    /// AppShell sets its primary renderer's scale to the density it lays
    /// out with, at construction and on every density change, so a host
    /// sets it only for the renderers of other surfaces. Renderers that take
    /// the scale with each draw ignore it.
    fn set_root_scale(&mut self, _scale: f32) {}

    /// The scene that holds the render graph and the hit targets.
    fn scene(&self) -> &Self::Scene;
    /// Mutable access to the scene.
    fn scene_mut(&mut self) -> &mut Self::Scene;

    /// Rebuilds the scene from `layout_tree` for a viewport of the given size.
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

    /// Updates the scene for the nodes in `updates` and refreshes the hit targets. The default
    /// implementation rebuilds the whole scene.
    fn update_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        viewport: Size,
        updates: SceneUpdates<'_>,
    ) -> Result<(), Self::Error> {
        let _ = updates;
        self.rebuild_scene_from_applier(applier, root, viewport)
    }

    /// Updates the scene when only what is drawn changed. Hit targets are refreshed only when their
    /// geometry changed. The default implementation calls `update_scene_from_applier`.
    fn update_visual_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        viewport: Size,
        updates: SceneUpdates<'_>,
    ) -> Result<(), Self::Error> {
        self.update_scene_from_applier(applier, root, viewport, updates)
    }

    /// Draw a development overlay (e.g., FPS counter) on top of the scene.
    ///
    /// This is called after rebuild_scene when dev options are enabled.
    /// The text is drawn directly by the renderer without affecting composition.
    ///
    /// Default implementation does nothing.
    fn draw_dev_overlay(&mut self, _text: &str, _viewport: Size) {}

    /// Replaces the developer inspector graph outside the application scene.
    ///
    /// Passing `None` removes the inspector. Its primitives must not participate
    /// in application hit testing, layout, or accessibility.
    fn set_inspector_overlay(&mut self, _graph: Option<graph::RenderGraph>) {}

    /// Returns whether renderer-side cache materialization needs a visible follow-up frame.
    fn needs_frame_warmup(&self) -> bool {
        false
    }

    /// Returns whether the last frame drew a placeholder for an effect whose
    /// pipelines are still compiling, so its picture is not final yet; the
    /// renderer asks for the frame that replaces it once they land.
    fn awaits_pipelines(&self) -> bool {
        false
    }
}
