#![doc = include_str!("../README.md")]

mod draw;
mod pipeline;
pub mod scene;
pub mod sprites;
pub mod style;

use cranpose_render_common::{
    RenderScene, Renderer, SceneUpdates,
    text_measure::{CachedFontTextMeasurer, SoftwareTextResources},
};
use cranpose_ui::LayoutTree;
use cranpose_ui_graphics::Size;
pub use draw::draw_scene;
pub use scene::{HitRegion, Scene};

#[derive(Debug)]
pub enum PixelsRendererError {
    Layout(String),
}

pub struct PixelsRenderer {
    scene: Scene,
    text_resources: SoftwareTextResources,
    /// The app context this renderer gave its text measurer.
    app_context: Option<std::rc::Weak<cranpose_ui::AppContext>>,
}

impl Default for PixelsRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl PixelsRenderer {
    pub fn new() -> Self {
        Self {
            scene: Scene::new(),
            text_resources: SoftwareTextResources::default(),
            app_context: None,
        }
    }

    pub fn draw(&self, frame: &mut [u8], width: u32, height: u32) {
        draw::draw_scene_with_text_resources(
            frame,
            width,
            height,
            &self.scene,
            &self.text_resources,
        );
    }

    /// Rasterizes logical scene coordinates at `scale` device pixels per point.
    /// Hit regions stay in logical coordinates. Invalid scales use 1.0.
    pub fn draw_scaled(&self, frame: &mut [u8], width: u32, height: u32, scale: f32) {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        draw::draw_scene_scaled(
            frame,
            width,
            height,
            &self.scene,
            &self.text_resources,
            scale,
        );
    }
}

impl PixelsRenderer {
    /// The scene at `scale` device pixels per point as sprites for a GPU
    /// compositor, into `frame`, which keeps its textures from the frames
    /// before. Invalid scales use 1.0.
    pub fn compose_sprites(
        &self,
        frame: &mut sprites::SpriteFrame,
        width: u32,
        height: u32,
        scale: f32,
    ) {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        let scene = match self.scene.graph.as_ref() {
            Some(graph) => pipeline::build_raster_scene(graph, self.scene.diagnostics(), scale),
            None => scene::RasterScene::new(),
        };
        frame.compose(
            &scene,
            width,
            height,
            self.scene.diagnostics(),
            &self.text_resources,
        );
    }
}

impl Renderer for PixelsRenderer {
    type Scene = Scene;
    type Error = PixelsRendererError;

    fn attach_app_context_services(&mut self, app_context: &cranpose_ui::AppContext) {
        app_context.set_text_measurer(CachedFontTextMeasurer::with_text_resources(
            self.text_resources.clone(),
            64,
        ));
        self.app_context = Some(app_context.downgrade());
    }

    #[cfg(feature = "installed-fonts")]
    fn apply_installed_fonts(&mut self, app_context: &cranpose_ui::AppContext) {
        let Some(resources) = self.text_resources.with_installed_faces() else {
            return;
        };
        self.text_resources = resources;
        let attached = self
            .app_context
            .as_ref()
            .and_then(std::rc::Weak::upgrade)
            .is_some_and(|attached| std::ptr::eq(&*attached, app_context));
        if attached {
            self.attach_app_context_services(app_context);
        }
    }

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        layout_tree: &LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.clear();
        pipeline::render_layout_tree(layout_tree.root(), &mut self.scene);
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.rebuild_from_applier(applier, root);
        Ok(())
    }

    fn update_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
        updates: SceneUpdates<'_>,
    ) -> Result<(), Self::Error> {
        self.scene.update_from_applier(applier, root, updates, true);
        Ok(())
    }

    fn update_visual_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
        updates: SceneUpdates<'_>,
    ) -> Result<(), Self::Error> {
        self.scene
            .update_from_applier(applier, root, updates, false);
        Ok(())
    }
}
