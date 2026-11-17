// The pipelines a scene's pass draws with, for each count of samples it
// takes in each pixel, and the parts they are built from.

use std::sync::Arc;

use cranpose_ui_graphics::space::Multisample;

use super::GpuRenderer;
use crate::{
    fixed_pipeline::{FixedPipeline, FixedPipelineJob},
    pipeline_compiler::CompilerSend,
    scene_pass::{SAMPLES, ScenePipeline, ScenePipelineParts, ShadedVariant, VolumeVariant},
};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "scene pipelines in their file"
)]
impl GpuRenderer {
    // Hands `visit` every scene pipeline with the job that builds it.
    pub(super) fn for_each_scene_pipeline(
        &self,
        visit: &mut dyn FnMut(&FixedPipeline, &dyn Fn() -> FixedPipelineJob),
    ) {
        for samples in SAMPLES {
            for kind in ScenePipeline::ALL {
                visit(self.scene_pass.pipeline(kind, samples), &|| {
                    Box::new(self.scene_surface_pipeline_job(kind, samples))
                });
            }
        }
    }

    // A scene pipeline for a scene's pass of `samples`, or `None` while it
    // compiles: the draws it takes are left out, and the frame after it
    // lands draws them.
    pub(crate) fn scene_surface_pipeline(
        &self,
        kind: ScenePipeline,
        samples: Multisample,
    ) -> Option<&wgpu::RenderPipeline> {
        self.pipelines.ready_or_left_out(
            &self.frame_stats,
            self.scene_pass.pipeline(kind, samples),
            || self.scene_surface_pipeline_job(kind, samples),
        )
    }

    // The pipeline of `variant` and `samples` of a volume shader a frame
    // noted; `None` while it compiles, when the frame after it lands draws
    // the volumes, or when the shader does not build.
    pub(crate) fn scene_volume_pipeline(
        &self,
        shader: &Arc<str>,
        (variant, samples): (VolumeVariant, Multisample),
    ) -> Option<&wgpu::RenderPipeline> {
        let kept = self.scene_pass.volume(shader)?;
        self.pipelines.app_shader_pipeline(
            &self.frame_stats,
            kept.pipeline(variant as usize, samples),
            || {
                crate::scene_pass::volume_pipeline_job(
                    &self.device,
                    self.pipelines.pipeline_cache.clone(),
                    (
                        self.pipelines.composition_format,
                        self.pipelines.adapter_backend,
                    ),
                    (variant, samples),
                    (self.scene_pipeline_parts(), kept),
                )
            },
        )
    }

    // The pipeline of `variant` and `samples` of a shaded surface's shader
    // a frame noted; `None` while it compiles, when the frame after it
    // lands draws the surfaces with it, or when the shader does not build.
    pub(crate) fn scene_shaded_pipeline(
        &self,
        shader: &Arc<str>,
        (variant, samples): (ShadedVariant, Multisample),
    ) -> Option<&wgpu::RenderPipeline> {
        let kept = self.scene_pass.shaded(shader)?;
        self.pipelines.app_shader_pipeline(
            &self.frame_stats,
            kept.pipeline(variant as usize, samples),
            || {
                crate::scene_pass::shaded_pipeline_job(
                    &self.device,
                    self.pipelines.pipeline_cache.clone(),
                    (
                        self.pipelines.composition_format,
                        self.pipelines.adapter_backend,
                    ),
                    (variant, samples),
                    (self.scene_pipeline_parts(), kept),
                )
            },
        )
    }

    // The bind group layout of a scene surface's uniforms.
    pub(crate) fn scene_uniform_layout(&self) -> &wgpu::BindGroupLayout {
        &self.scene_pipeline_parts().uniform_layout
    }

    // What a scene binds in its shadow map's place when it casts none.
    pub(crate) fn scene_no_shadow(&self) -> &wgpu::BindGroup {
        &self.scene_pipeline_parts().no_shadow
    }

    fn scene_pipeline_parts(&self) -> &ScenePipelineParts {
        self.scene_pass.parts(
            &self.device,
            self.pipelines.adapter_backend,
            &self.effect_renderer.effect_texture_bind_group_layout,
        )
    }

    fn scene_surface_pipeline_job(
        &self,
        kind: ScenePipeline,
        samples: Multisample,
    ) -> impl FnOnce() -> wgpu::RenderPipeline + CompilerSend + 'static {
        crate::scene_pass::scene_pipeline_job(
            &self.device,
            self.pipelines.pipeline_cache.clone(),
            (self.pipelines.composition_format, kind, samples),
            self.scene_pipeline_parts(),
        )
    }
}
