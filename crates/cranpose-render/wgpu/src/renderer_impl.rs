use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::{
    RenderScene, Renderer,
    graph::RenderGraph,
    software_text_raster::{SoftwareTextFontSet, SoftwareTextMeasurer},
};
use cranpose_ui::LayoutTree;
use cranpose_ui_graphics::Size;

use crate::{PresentBackend, Scene, WgpuRenderer, WgpuRendererError, pipeline};

/// Gives `app_context` the measurer its text lays out with, over `fonts`.
fn install_text_measurer(app_context: &cranpose_ui::AppContext, fonts: &SoftwareTextFontSet) {
    app_context.set_text_measurer(SoftwareTextMeasurer::from_font_set(fonts.clone(), 8192));
}

impl Renderer for WgpuRenderer {
    type Scene = Scene;
    type Error = WgpuRendererError;

    fn attach_app_context_services(&mut self, app_context: &cranpose_ui::AppContext) {
        install_text_measurer(app_context, &self.frontend.text_fonts);
        self.frontend.app_context = Some(app_context.downgrade());
    }

    #[cfg(feature = "installed-fonts")]
    fn apply_installed_fonts(&mut self, app_context: &cranpose_ui::AppContext) {
        let Some(fonts) = self.frontend.text_fonts.with_installed_faces() else {
            return;
        };
        let attached = self
            .frontend
            .app_context
            .as_ref()
            .and_then(std::rc::Weak::upgrade)
            .is_some_and(|attached| std::ptr::eq(&*attached, app_context));
        if attached {
            install_text_measurer(app_context, &fonts);
        }
        self.frontend.text_state =
            crate::text_system::TextSystemState::from_font_set(fonts.clone());
        self.frontend.text_fonts = fonts;
    }

    fn set_root_scale(&mut self, scale: f32) {
        self.frontend.root_scale = scale;
    }

    fn scene(&self) -> &Self::Scene {
        &self.frontend.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.frontend.scene
    }

    fn rebuild_scene(
        &mut self,
        layout_tree: &LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.frontend.scene.clear();
        self.frontend.clear_fps_overlay();
        pipeline::render_layout_tree(layout_tree.root(), &mut self.frontend.scene);
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.frontend.clear_fps_overlay();
        self.frontend.scene.rebuild_from_applier(applier, root);
        Ok(())
    }

    fn update_scene_from_applier(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        _viewport: Size,
        updates: cranpose_render_common::SceneUpdates<'_>,
    ) -> Result<(), Self::Error> {
        if updates.is_empty() {
            self.frontend.clear_fps_overlay();
        }
        self.frontend
            .scene
            .update_from_applier(applier, root, updates);
        Ok(())
    }

    fn draw_dev_overlay(&mut self, text: &str, viewport: Size) {
        const DEV_OVERLAY_NODE_ID: NodeId = NodeId(usize::MAX);
        let key = cranpose_render_common::dev_overlay::DevOverlayKey::new(text, viewport);
        if self.frontend.dev_overlay_graph.is_some()
            && self.frontend.dev_overlay_cache.as_ref() == Some(&key)
        {
            return;
        }
        self.frontend.fps_overlay_graph = Some(
            cranpose_render_common::dev_overlay::build_dev_overlay_graph(
                text,
                viewport,
                DEV_OVERLAY_NODE_ID,
            ),
        );
        self.frontend.dev_overlay_cache = Some(key);
        self.frontend.refresh_dev_overlay();
    }

    fn note_held_still(&mut self) {
        self.frontend.held_still = true;
    }

    fn note_pointer_held(&mut self) {
        self.frontend.pointer_held = true;
    }

    fn set_inspector_overlay(&mut self, graph: Option<RenderGraph>) {
        self.frontend.inspector_overlay_graph = graph;
        self.frontend.refresh_dev_overlay();
    }

    fn needs_frame_warmup(&self) -> bool {
        match &self.backend {
            PresentBackend::Sync(sync) => sync.renderer.needs_frame_warmup(),
            #[cfg(not(target_arch = "wasm32"))]
            PresentBackend::Threaded(handle) => handle
                .status()
                .needs_frame_warmup
                .load(std::sync::atomic::Ordering::Relaxed),
            PresentBackend::None => false,
        }
    }

    fn awaits_pipelines(&self) -> bool {
        match &self.backend {
            PresentBackend::Sync(sync) => sync.renderer.pipelines.awaits_pipelines(),
            #[cfg(not(target_arch = "wasm32"))]
            PresentBackend::Threaded(handle) => {
                let status = handle.status();
                status
                    .drew_placeholder
                    .load(std::sync::atomic::Ordering::Relaxed)
                    && status
                        .landing
                        .get()
                        .is_some_and(|landing| landing.in_flight())
            }
            PresentBackend::None => false,
        }
    }
}
