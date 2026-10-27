use cranpose_render_common::geometry::blur_scratch_block;
use cranpose_ui_graphics::RenderEffect;

use super::ChainedSubstrates;
use crate::{
    frame_graph::{FrameCommandRecorder, FrameTextureDescriptor},
    offscreen::OffscreenTarget,
};

pub(crate) trait EffectScratchTargetProvider<'target> {
    fn next(&mut self) -> Result<&'target OffscreenTarget, String>;
    fn assert_consumed(&self) -> Result<(), String>;
}

pub(crate) struct RecordedEffectScratchTargets {
    targets: Vec<RecordedEffectScratchTarget>,
}

struct RecordedEffectScratchTarget {
    descriptor: FrameTextureDescriptor,
    target: OffscreenTarget,
}

impl RecordedEffectScratchTargets {
    pub(super) fn new() -> Self {
        Self {
            targets: Vec::new(),
        }
    }

    fn push(&mut self, descriptor: FrameTextureDescriptor, target: OffscreenTarget) {
        self.targets
            .push(RecordedEffectScratchTarget { descriptor, target });
    }

    pub(crate) fn release_into<C: FrameCommandRecorder>(self, recorder: &mut C) {
        for scratch in self.targets {
            recorder.release_transient_offscreen(scratch.descriptor, scratch.target);
        }
    }

    pub(crate) fn refs(&self) -> RecordedEffectScratchTargetRefs<'_> {
        RecordedEffectScratchTargetRefs {
            targets: &self.targets,
            next: 0,
        }
    }
}

pub(crate) struct RecordedEffectScratchTargetRefs<'a> {
    targets: &'a [RecordedEffectScratchTarget],
    next: usize,
}

impl<'a> EffectScratchTargetProvider<'a> for RecordedEffectScratchTargetRefs<'a> {
    fn next(&mut self) -> Result<&'a OffscreenTarget, String> {
        let index = self.next;
        let target = self
            .targets
            .get(index)
            .map(|scratch| &scratch.target)
            .ok_or_else(|| format!("render effect scratch target {index} was not acquired"))?;
        self.next += 1;
        Ok(target)
    }

    fn assert_consumed(&self) -> Result<(), String> {
        if self.next == self.targets.len() {
            Ok(())
        } else {
            Err(format!(
                "render effect acquired {} scratch targets but consumed {}",
                self.targets.len(),
                self.next
            ))
        }
    }
}

pub(super) fn acquire_recorded_effect_scratch_textures_into<C: FrameCommandRecorder>(
    recorder: &mut C,
    device: &wgpu::Device,
    effect: &RenderEffect,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    targets: &mut RecordedEffectScratchTargets,
) {
    match effect {
        RenderEffect::Blur {
            radius_x, radius_y, ..
        } => {
            if *radius_x > 0.0 || *radius_y > 0.0 {
                let (scratch_width, scratch_height) =
                    blur_scratch_size(*radius_x, *radius_y, width, height);
                let descriptor = FrameTextureDescriptor::render_attachment(
                    "Render Effect Blur Scratch",
                    scratch_width,
                    scratch_height,
                    format,
                );
                let target = recorder.acquire_transient_offscreen(device, descriptor);
                targets.push(descriptor, target);
            }
        }
        RenderEffect::Offset { .. } => {}
        RenderEffect::Shader { shader } => {
            if !shader.substrates().is_empty() {
                let layout = ChainedSubstrates::new(shader, width, height);
                for _ in 0..2 {
                    let descriptor = FrameTextureDescriptor::render_attachment(
                        "Chained Shader Substrates",
                        layout.width,
                        layout.height,
                        format,
                    );
                    let target = recorder.acquire_transient_offscreen(device, descriptor);
                    targets.push(descriptor, target);
                }
            }
        }
        RenderEffect::Chain { first, second } => {
            let descriptor = FrameTextureDescriptor::render_attachment(
                "Render Effect Chain Scratch",
                width,
                height,
                format,
            );
            let target = recorder.acquire_transient_offscreen(device, descriptor);
            targets.push(descriptor, target);
            acquire_recorded_effect_scratch_textures_into(
                recorder, device, first, width, height, format, targets,
            );
            acquire_recorded_effect_scratch_textures_into(
                recorder, device, second, width, height, format, targets,
            );
        }
    }
}

pub(crate) fn blur_scratch_size(
    radius_x: f32,
    radius_y: f32,
    width: u32,
    height: u32,
) -> (u32, u32) {
    scratch_size_at(blur_scratch_block(radius_x.max(radius_y)), width, height)
}

/// The scratch a substrate blur of `radius_px` runs at: a low-frequency copy
/// read once per fragment tolerates a coarser grid than a blur that is the
/// picture, so the scale doubles at half the radius the blur's does.
pub(crate) fn substrate_scratch_size(radius_px: f32, width: u32, height: u32) -> (u32, u32) {
    let scale = if radius_px < 3.0 {
        1
    } else if radius_px < 8.0 {
        2
    } else {
        4
    };
    scratch_size_at(scale, width, height)
}

fn scratch_size_at(mut scale: u32, width: u32, height: u32) -> (u32, u32) {
    while scale > 1 && (width / scale < 16 || height / scale < 16) {
        scale /= 2;
    }
    if scale <= 1 {
        return (width, height);
    }
    (width.div_ceil(scale).max(1), height.div_ceil(scale).max(1))
}
