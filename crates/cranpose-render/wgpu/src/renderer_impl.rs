use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::{Renderer, scene_store::OverlayLayer, text_fonts::TextFonts};
use cranpose_ui_graphics::Size;

use crate::{PresentBackend, Scene, WgpuRenderer, WgpuRendererError};

/// Gives `app_context` the fonts its text lays out with.
fn install_text_fonts(app_context: &cranpose_ui::AppContext, fonts: &TextFonts) {
    app_context.set_text_fonts(fonts.resolver());
}

impl Renderer for WgpuRenderer {
    type Scene = Scene;
    type Error = WgpuRendererError;

    fn attach_app_context_services(&mut self, app_context: &cranpose_ui::AppContext) {
        install_text_fonts(app_context, &self.frontend.text_fonts);
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
            install_text_fonts(app_context, &fonts);
        }
        self.frontend.text_fonts = fonts;
    }

    fn trim(&mut self) {
        self.frontend.trim();
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

    fn draw_dev_overlay(&mut self, overlay: &cranpose_ui::PreparedTextLayout, viewport: Size) {
        const DEV_OVERLAY_NODE_ID: NodeId = NodeId(usize::MAX);
        let key =
            cranpose_render_common::dev_overlay::DevOverlayKey::new(overlay.text.text(), viewport);
        if self.frontend.dev_overlay.root().is_some()
            && self.frontend.dev_overlay_cache.as_ref() == Some(&key)
        {
            return;
        }
        self.frontend.fps_overlay = Some(cranpose_render_common::dev_overlay::build_fps_overlay(
            overlay,
            viewport,
            DEV_OVERLAY_NODE_ID,
        ));
        self.frontend.dev_overlay_cache = Some(key);
        self.frontend.refresh_dev_overlay();
    }

    fn note_held_still(&mut self) {
        self.frontend.held_still = true;
    }

    fn note_pointer_held(&mut self) {
        self.frontend.pointer_held = true;
    }

    #[cfg(target_arch = "wasm32")]
    fn build_while_idle(&mut self) -> bool {
        let PresentBackend::Sync(sync) = &self.backend else {
            return false;
        };
        let renderer = &sync.renderer;
        renderer
            .pipelines
            .compiler
            .build_while_idle(&renderer.device, &renderer.queue)
    }

    fn set_inspector_overlay(&mut self, overlay: Option<OverlayLayer>) {
        self.frontend.inspector_overlay = overlay;
        self.frontend.refresh_dev_overlay();
    }

    #[cfg(feature = "layer-tree")]
    fn set_overlay_tree(&mut self, overlay: Option<cranpose_render_common::graph::LayerNode>) {
        self.frontend.describe_overlay_tree(overlay);
    }

    fn needs_frame_warmup(&self) -> bool {
        match &self.backend {
            PresentBackend::Sync(sync) => sync.renderer.needs_frame_warmup(),
            #[cfg(not(target_arch = "wasm32"))]
            PresentBackend::Threaded(handle) => handle.status().warmup_due(),
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
                    && status.landing_is(crate::pipeline_compiler::Landing::in_flight)
            }
            PresentBackend::None => false,
        }
    }
}
