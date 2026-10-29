use cranpose_ui::Axis;
use cranpose_ui_graphics::TileMode;

use super::{BlurUniforms, EffectRenderer, blur_pipelines::BlurPipeline, tile_mode_uniform_value};
use crate::{frame_graph::UniformUpload, offscreen::OffscreenTarget};

/// The block of source texels one pixel of a blur's scratch stands for: one
/// when `scratch` is `source`'s size.
pub(super) fn blur_block(source: (u32, u32, u32, u32), scratch: (u32, u32, u32, u32)) -> u32 {
    (source.2 as f32 / scratch.2.max(1) as f32).round().max(1.0) as u32
}

#[derive(Clone, Copy)]
pub(super) struct BlurDraw {
    pub(super) source: BlurSource,
    pub(super) uniforms: BlurUniformSpec,
    pub(super) filter: BlurFilter,
    pub(super) scissor: Option<(u32, u32, u32, u32)>,
}

#[derive(Clone, Copy)]
pub(super) enum BlurSource {
    Primary,
    Secondary,
}

pub(super) struct BlurSourceSet<'a> {
    pub(super) primary: &'a OffscreenTarget,
    pub(super) secondary: &'a OffscreenTarget,
}

impl<'a> BlurSourceSet<'a> {
    pub(super) fn single(target: &'a OffscreenTarget) -> Self {
        BlurSourceSet {
            primary: target,
            secondary: target,
        }
    }

    pub(super) fn resolve(&self, source: BlurSource) -> &OffscreenTarget {
        match source {
            BlurSource::Primary => self.primary,
            BlurSource::Secondary => self.secondary,
        }
    }
}

pub(super) struct PreparedBlurDraw {
    pub(super) source: BlurSource,
    pub(super) uniform: UniformUpload,
    pub(super) pipeline: BlurPipeline,
    pub(super) scissor: Option<(u32, u32, u32, u32)>,
}

#[derive(Clone, Copy)]
pub(super) struct BlurUniformSpec {
    pub(super) axis: Axis,
    pub(super) sampled: (u32, u32),
    pub(super) source: [f32; 4],
    pub(super) dest: [f32; 4],
    pub(super) radius: (f32, f32),
    pub(super) tile_mode: TileMode,
}

#[derive(Clone, Copy)]
pub(super) enum BlurFilter {
    Kernel,
    Downsample(u32),
    Mean,
}

impl BlurDraw {
    pub(super) fn uniforms(&self, renderer: &EffectRenderer) -> BlurUniforms {
        let spec = self.uniforms;
        renderer.blur_uniforms(
            spec.axis,
            spec.sampled,
            spec.source,
            spec.dest,
            spec.radius,
            spec.tile_mode,
        )
    }

    pub(super) fn pipeline(&self) -> BlurPipeline {
        let tile_mode = tile_mode_uniform_value(self.uniforms.tile_mode) as usize;
        match self.filter {
            BlurFilter::Downsample(block) => BlurPipeline::Downsample { block, tile_mode },
            BlurFilter::Kernel => BlurPipeline::Kernel { tile_mode },
            BlurFilter::Mean => BlurPipeline::Mean,
        }
    }
}
