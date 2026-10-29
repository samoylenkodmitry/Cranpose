use std::cell::Cell;

use super::{BLUR_TILE_MODES, ShaderEntry, fullscreen_pipeline_job};
use crate::{
    fixed_pipeline::{FixedPipeline, FixedPipelineJob},
    pipeline_compiler::{CompileLane, PipelineCompiler},
    pipeline_recorder::PipelineRecorder,
    shared_shader::SharedShader,
};

/// The blocks of source texels a blur's downsample averages per scratch
/// pixel: the downscales `blur_scratch_size` chooses.
const BLUR_DOWNSAMPLE_BLOCKS: [u32; 2] = [2, 4];
const BLUR_LABELS: [&str; BLUR_TILE_MODES.len()] = [
    "effect/blur/clamp",
    "effect/blur/repeated",
    "effect/blur/mirror",
    "effect/blur/decal",
];
const BLUR_DOWNSAMPLE_LABELS: [[&str; BLUR_DOWNSAMPLE_BLOCKS.len()]; BLUR_TILE_MODES.len()] = [
    [
        "effect/blur-downsample/clamp/2",
        "effect/blur-downsample/clamp/4",
    ],
    [
        "effect/blur-downsample/repeated/2",
        "effect/blur-downsample/repeated/4",
    ],
    [
        "effect/blur-downsample/mirror/2",
        "effect/blur-downsample/mirror/4",
    ],
    [
        "effect/blur-downsample/decal/2",
        "effect/blur-downsample/decal/4",
    ],
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BlurPipeline {
    Mean,
    Downsample { block: u32, tile_mode: usize },
    Kernel { tile_mode: usize },
}

impl BlurPipeline {
    /// The tile mode whose blur family this pipeline belongs to: its
    /// kernel and its downsample of each block. The mean has none.
    pub(super) fn family(self) -> Option<usize> {
        match self {
            Self::Mean => None,
            Self::Downsample { tile_mode, .. } | Self::Kernel { tile_mode } => Some(tile_mode),
        }
    }
}

/// The pipelines of the blur family of `tile_mode`.
pub(super) fn blur_family(tile_mode: usize) -> impl Iterator<Item = BlurPipeline> {
    std::iter::once(BlurPipeline::Kernel { tile_mode }).chain(
        BLUR_DOWNSAMPLE_BLOCKS
            .into_iter()
            .map(move |block| BlurPipeline::Downsample { block, tile_mode }),
    )
}

// The blur programs: a kernel per tile mode, a downsample per tile mode and
// block, and the mean, with what builds them.
pub(super) struct BlurPipelines {
    pub(super) shader: SharedShader,
    kernels: [FixedPipeline; BLUR_TILE_MODES.len()],
    downsamples: [[FixedPipeline; BLUR_DOWNSAMPLE_BLOCKS.len()]; BLUR_TILE_MODES.len()],
    mean: FixedPipeline,
    // One bit per tile mode whose blur family is queued for warm-up.
    families_queued: Cell<u8>,
    compiler: PipelineCompiler,
    pipeline_cache: Option<wgpu::PipelineCache>,
    recorder: PipelineRecorder,
    surface_format: wgpu::TextureFormat,
    adapter_backend: wgpu::Backend,
}

impl BlurPipelines {
    pub(super) fn new(
        shader: SharedShader,
        compiler: PipelineCompiler,
        pipeline_cache: Option<wgpu::PipelineCache>,
        recorder: PipelineRecorder,
        surface_format: wgpu::TextureFormat,
        adapter_backend: wgpu::Backend,
    ) -> Self {
        Self {
            shader,
            kernels: BLUR_LABELS.map(FixedPipeline::new),
            downsamples: BLUR_DOWNSAMPLE_LABELS.map(|labels| labels.map(FixedPipeline::new)),
            mean: FixedPipeline::new("effect/mean"),
            families_queued: Cell::new(0),
            compiler,
            pipeline_cache,
            recorder,
            surface_format,
            adapter_backend,
        }
    }

    pub(super) fn ready(&self, device: &wgpu::Device, pipeline: BlurPipeline) -> bool {
        self.resource(pipeline)
            .ready_or_queue(&self.compiler, self.adapter_backend, || {
                self.job(device, pipeline)
            })
    }

    pub(super) fn resource(&self, pipeline: BlurPipeline) -> &FixedPipeline {
        match pipeline {
            BlurPipeline::Mean => &self.mean,
            BlurPipeline::Kernel { tile_mode } => &self.kernels[tile_mode],
            BlurPipeline::Downsample { block, tile_mode } => {
                let index = BLUR_DOWNSAMPLE_BLOCKS
                    .iter()
                    .position(|candidate| *candidate == block)
                    .unwrap_or_else(|| {
                        panic!("a blur downsample block of {block}; the scratch is 2 or 4 to 1")
                    });
                &self.downsamples[tile_mode][index]
            }
        }
    }

    pub(super) fn job(&self, device: &wgpu::Device, pipeline: BlurPipeline) -> FixedPipelineJob {
        match pipeline {
            BlurPipeline::Mean => self.mean_job(device),
            BlurPipeline::Kernel { tile_mode } => self.kernel_job(device, tile_mode),
            BlurPipeline::Downsample { block, tile_mode } => {
                self.downsample_job(device, block, tile_mode)
            }
        }
    }

    /// The pipeline of a blur draw, built here if nothing has built it yet.
    /// The first draw of a tile mode's blur family queues the rest of the
    /// family for warm-up: the downsample block follows the radius, so an
    /// effect that blurs asks for the other block once its radius crosses
    /// a threshold, often mid-scroll.
    pub(super) fn for_draw(
        &self,
        device: &wgpu::Device,
        pipeline: BlurPipeline,
    ) -> &wgpu::RenderPipeline {
        if let Some(tile_mode) = pipeline.family() {
            let bit = 1u8 << tile_mode;
            let queued = self.families_queued.get();
            if queued & bit == 0 {
                self.families_queued.set(queued | bit);
                for sibling in blur_family(tile_mode).filter(|sibling| *sibling != pipeline) {
                    self.resource(sibling).queue(
                        &self.compiler,
                        CompileLane::WarmUp,
                        self.adapter_backend,
                        self.job(device, sibling),
                    );
                }
            }
        }
        self.resource(pipeline)
            .for_draw(&self.recorder, self.adapter_backend, || {
                self.job(device, pipeline)()
            })
    }

    fn kernel_job(&self, device: &wgpu::Device, tile_mode: usize) -> FixedPipelineJob {
        fullscreen_pipeline_job(
            device,
            self.pipeline_cache.as_ref(),
            "Blur Pipeline",
            ShaderEntry {
                shader: &self.shader,
                entries: (crate::render::FULLSCREEN_VS, "blur_fs"),
                constants: &[("BLUR_TILE_MODE", tile_mode as f64)],
            },
            self.surface_format,
            (wgpu::BlendState::REPLACE, None),
        )
    }

    fn mean_job(&self, device: &wgpu::Device) -> FixedPipelineJob {
        fullscreen_pipeline_job(
            device,
            self.pipeline_cache.as_ref(),
            "Mean Pipeline",
            ShaderEntry {
                shader: &self.shader,
                entries: (crate::render::FULLSCREEN_VS, "blur_mean_fs"),
                constants: &[],
            },
            self.surface_format,
            (wgpu::BlendState::REPLACE, None),
        )
    }

    fn downsample_job(
        &self,
        device: &wgpu::Device,
        block: u32,
        tile_mode: usize,
    ) -> FixedPipelineJob {
        fullscreen_pipeline_job(
            device,
            self.pipeline_cache.as_ref(),
            "Blur Downsample Pipeline",
            ShaderEntry {
                shader: &self.shader,
                entries: (crate::render::FULLSCREEN_VS, "blur_downsample_fs"),
                constants: &[
                    ("BLUR_BLOCK", f64::from(block)),
                    ("BLUR_TILE_MODE", tile_mode as f64),
                ],
            },
            self.surface_format,
            (wgpu::BlendState::REPLACE, None),
        )
    }
}
