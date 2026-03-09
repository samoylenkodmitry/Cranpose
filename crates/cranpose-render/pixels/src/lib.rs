mod draw;
mod pipeline;
pub mod scene;
pub mod style;

use cranpose_render_common::{RenderScene, Renderer};
use cranpose_ui::{set_text_measurer, LayoutTree};
use cranpose_ui_graphics::Size;

pub use draw::draw_scene;
pub use scene::{HitRegion, Scene};

#[derive(Debug)]
pub enum PixelsRendererError {
    Layout(String),
}

pub struct PixelsRenderer {
    scene: Scene,
}

impl Default for PixelsRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl PixelsRenderer {
    pub fn new() -> Self {
        set_text_measurer(draw::CachedFontTextMeasurer::new(64));
        Self {
            scene: Scene::new(),
        }
    }

    pub fn draw(&self, frame: &mut [u8], width: u32, height: u32) {
        draw::draw_scene(frame, width, height, &self.scene);
    }
}

impl Renderer for PixelsRenderer {
    type Scene = Scene;
    type Error = PixelsRendererError;

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
        self.scene.clear();
        pipeline::render_from_applier(applier, root, &mut self.scene);
        Ok(())
    }
}
