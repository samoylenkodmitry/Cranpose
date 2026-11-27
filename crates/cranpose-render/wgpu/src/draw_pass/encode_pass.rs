use super::{
    Batch, PassPrep, PassScratch, PassSegment, PassTarget, RunCalls, SegmentLayout, TrackedPass,
    batch::{PassCmds, group_glyph_batches, stage_scissor},
    interiors_first::takes_depth,
    mixed_turns::turns_mixed,
    pass_buffers::PassBuffers,
};
use crate::{
    frame_graph::FrameCommandRecorder,
    lazy_resource::Pending,
    pipeline_compiler::CompileLane,
    render::{GpuRenderer, ImageSlot, PassDepth, PassFrame, RunStage},
    run_store::RunDrawCall,
    shape_pipelines::PipelineDemand,
};

impl GpuRenderer {
    /// Uploads the quads a prepared pass drew into its scratch: image
    /// vertices, shadow instances and glyph instances, each when there are
    /// any.
    fn upload_pass_buffers<C: FrameCommandRecorder>(
        &self,
        recorder: &mut C,
        scratch: &PassScratch,
    ) -> PassBuffers {
        let builder = &scratch.meshes.builder;
        PassBuffers {
            meshes: (!builder.is_empty()).then(|| crate::mesh_fallback::MeshBuffers {
                vertices: recorder.upload_buffer(
                    crate::frame_graph::UploadAllocatorSpec::vertex(
                        "Mesh Fallback Vertex Buffer",
                        size_of::<crate::mesh_fallback::MeshVertex>() as u64,
                    ),
                    &self.device,
                    bytemuck::cast_slice(builder.vertices()),
                ),
                indices: recorder.upload_buffer(
                    crate::frame_graph::UploadAllocatorSpec::index(
                        "Mesh Fallback Index Buffer",
                        size_of::<u16>() as u64,
                    ),
                    &self.device,
                    bytemuck::cast_slice(builder.indices()),
                ),
            }),
            images: (!scratch.images.indices.is_empty()).then(|| {
                ImageSlot::upload(
                    recorder,
                    &self.device,
                    &scratch.images.vertices,
                    &scratch.images.indices,
                    &scratch.images.clips,
                )
            }),
            shadows: (!scratch.shadow_instances.is_empty())
                .then(|| self.upload_shadow_instances(recorder, &scratch.shadow_instances)),
            glyphs: (!scratch.glyph_instances.plain.is_empty())
                .then(|| self.upload_glyph_instances(recorder, &scratch.glyph_instances.plain)),
            turned_glyphs: (!scratch.glyph_instances.turned.is_empty())
                .then(|| self.upload_turned_glyphs(recorder, &scratch.glyph_instances.turned)),
            instant: scratch.instant.upload(&self.device),
        }
    }

    /// Draws the segments into the target as one render pass, ops and
    /// composites interleaved in z order. Returns whether anything was drawn;
    /// when nothing draws and the load op clears, a clear pass runs instead so
    /// the target still holds its base.
    pub(crate) fn encode_pass<'s, C: FrameCommandRecorder>(
        &mut self,
        recorder: &mut C,
        target: PassTarget<'_>,
        (segments, layout): (&'s [PassSegment<'s>], SegmentLayout),
        load_op: wgpu::LoadOp<wgpu::Color>,
        label: &'static str,
    ) -> Result<bool, String> {
        let mut scratch = self.take_pass_scratch();
        let (depth, depth_keys) = self.pass_depth(segments);
        let mut prep = PassPrep::new(
            recorder,
            (target, load_op),
            (depth, depth_keys),
            turns_mixed(segments),
            &mut scratch,
        );
        let prepared = prep.segments(self, (segments, layout), &mut scratch);
        prep.finish(self, &mut scratch);
        let mut parts = self.draw_parts();
        parts.text.flush_glyph_uploads(&mut parts.upload);
        self.prefetch_overlay_pipelines(
            &prep.batches,
            &scratch,
            (depth, self.pipelines.frame_lane()),
            &mut |_| {},
        );
        if let Some(depth_keys) = prep.depth_keys.take() {
            self.await_depth_pipelines(&prep.batches, &scratch, &depth_keys);
        }
        self.pipelines.ensure_run_pipelines(&mut prep.pipelines);
        let laid = (&scratch.glyph_instances, &mut scratch.instant);
        self.pipelines.lay_turned_glyphs(depth, laid.0, laid.1);
        let batches = prep.batches;
        scratch.arena_draws = prep.arena_draws;
        scratch.meshes = prep.meshes;
        scratch.pipelines = prep.pipelines;
        scratch.held = prep.held;
        let buffers = if prepared.is_ok() {
            self.upload_pass_buffers(recorder, &scratch)
        } else {
            PassBuffers::default()
        };
        let result = match prepared {
            Err(error) => Err(error),
            Ok(()) if batches.is_empty() => {
                if matches!(load_op, wgpu::LoadOp::Clear(_)) {
                    self.clear_target(recorder, target.view, load_op);
                }
                Ok(false)
            }
            Ok(()) => {
                let composite_draws = batches
                    .iter()
                    .filter(|batch| matches!(batch, Batch::Composite(_) | Batch::Shader(_)))
                    .count() as u32;
                if composite_draws > 0 {
                    self.effect_renderer.record_composite_pass();
                    self.frame_stats.add_draw_calls(composite_draws);
                }
                group_glyph_batches(&batches, &mut scratch);
                let draw_result = {
                    let frame = PassFrame {
                        size: (target.width, target.height),
                        depth,
                    };
                    let mut pass = self.begin_scene_pass(recorder, label, target, load_op, depth);
                    self.draw_batches(
                        &mut TrackedPass::new(&mut pass),
                        frame,
                        &batches,
                        PassCmds {
                            images: &scratch.images.cmds,
                            glyphs: &scratch.glyph_cmds,
                            arena: &scratch.arena_draws,
                            meshes: &scratch.meshes,
                        },
                        &buffers,
                    )
                };
                recorder.record_pass();
                draw_result.map(|()| true)
            }
        };
        drop(batches);
        self.pass_scratch = scratch;
        result
    }

    // The depth a pass over `segments` draws with, and the shape pipelines
    // it collects for a depth buffer when it could take one but its gate is
    // still shut.
    fn pass_depth(&self, segments: &[PassSegment<'_>]) -> (PassDepth, Option<PipelineDemand>) {
        let takes_depth = self.depth_gate.may() && takes_depth(segments);
        if takes_depth && self.depth_gate.is_open() {
            (PassDepth::On, None)
        } else {
            (PassDepth::Off, takes_depth.then(PipelineDemand::default))
        }
    }

    fn take_pass_scratch(&mut self) -> PassScratch {
        std::mem::take(&mut self.pass_scratch).cleared()
    }

    /// Starts on `lane` the builds of the image, glyph and shadow pipelines a
    /// prepared pass draws with, with a depth buffer or not, that are
    /// missing, handing `awaited` each still to land;
    /// [`GpuRenderer::ensure_run_pipelines`] starts its shapes' and the
    /// pass's composites start theirs as it prepares them. A launch's first
    /// frame needs about fifteen; built one after another where the pass
    /// reached each, they held the gauntlet's first frame 1.1 s on an Apple
    /// M5 with no Metal shader cached. Where the compiler's threads run, the
    /// pass leaves out a draw whose pipeline is still to land; elsewhere it
    /// builds each where it draws with it.
    fn prefetch_overlay_pipelines(
        &self,
        batches: &[Batch<'_>],
        scratch: &PassScratch,
        depth_lane: (PassDepth, CompileLane),
        awaited: &mut impl FnMut(Pending<wgpu::RenderPipeline>),
    ) {
        for batch in batches {
            match batch {
                Batch::Images {
                    cmds, blend_mode, ..
                } => self.pipelines.prefetch_image_pipelines(
                    &scratch.images.cmds[cmds.clone()],
                    *blend_mode,
                    depth_lane,
                    awaited,
                ),
                Batch::Glyphs { cmds, .. } => self.pipelines.prefetch_glyph_pipelines(
                    &scratch.glyph_cmds[cmds.clone()],
                    depth_lane,
                    awaited,
                ),
                Batch::Shadows { .. } => {
                    self.pipelines.prefetch_shadow_pipeline(depth_lane, awaited);
                }
                Batch::Composite(prepared) => self.effect_renderer.prefetch_composite_pipeline(
                    &self.device,
                    prepared,
                    depth_lane,
                    awaited,
                ),
                Batch::Projective(prepared) => self.effect_renderer.prefetch_projective_pipeline(
                    &self.device,
                    prepared,
                    depth_lane,
                    awaited,
                ),
                Batch::StoreRun { .. }
                | Batch::Arena { .. }
                | Batch::Shader(_)
                | Batch::Instant(_) => {}
            }
        }
    }

    /// Queues on the demanded lane the pipelines a pass drawn without the
    /// depth pre-pass it takes would have drawn with: its shapes' with
    /// `depth_keys` and the interiors they lay down, and its images', glyphs',
    /// shadows' and blits' with a depth buffer. The depth gate opens once they
    /// land.
    fn await_depth_pipelines(
        &mut self,
        batches: &[Batch<'_>],
        scratch: &PassScratch,
        depth_keys: &PipelineDemand,
    ) {
        let mut awaited = self.depth_gate.take_awaited();
        self.pipelines.demand_depth(depth_keys, &mut awaited);
        self.prefetch_overlay_pipelines(
            batches,
            scratch,
            (PassDepth::On, CompileLane::Demanded),
            &mut |pending| awaited.push(pending),
        );
        self.depth_gate.waited_for(awaited);
    }

    fn draw_batches(
        &mut self,
        pass: &mut TrackedPass<'_, '_>,
        frame: PassFrame,
        batches: &[Batch<'_>],
        cmds: PassCmds<'_>,
        buffers: &PassBuffers,
    ) -> Result<(), String> {
        if frame.depth.is_on() {
            for batch in batches.iter().rev() {
                self.draw_shape_batch(pass, batch, cmds.arena, None, frame, RunStage::Interiors)?;
            }
        }
        let meshes = buffers.run_meshes(cmds.meshes);
        for batch in batches {
            match batch {
                Batch::StoreRun { .. } | Batch::Arena { .. } => {
                    self.draw_shape_batch(pass, batch, cmds.arena, meshes, frame, RunStage::Paint)?;
                }
                Batch::Images {
                    cmds: range,
                    blend_mode,
                    uniform_slot,
                    scissor,
                } => {
                    let slot = buffers
                        .images
                        .as_ref()
                        .ok_or_else(|| "image batch without an image slot".to_string())?;
                    self.draw_image_cmds(
                        pass,
                        slot,
                        *uniform_slot,
                        &cmds.images[range.clone()],
                        (*blend_mode, frame.depth),
                        *scissor,
                    )?;
                }
                Batch::Glyphs {
                    cmds: range,
                    uniform_slot,
                    scissor,
                } => {
                    let stand_in = self.pipelines.instant_stand_in(frame.depth);
                    self.draw_glyph_cmds(
                        pass,
                        (buffers.glyphs.as_ref(), buffers.turned_glyphs(stand_in)),
                        *uniform_slot,
                        &cmds.glyphs[range.clone()],
                        *scissor,
                        frame,
                    )?;
                }
                Batch::Shadows {
                    instances,
                    uniform_slot,
                    scissor,
                } => {
                    let buffer = buffers
                        .shadows
                        .as_ref()
                        .ok_or_else(|| "shadow batch without shadow instances".to_string())?;
                    self.draw_shadow_instances(
                        pass,
                        buffer,
                        *uniform_slot,
                        instances.clone(),
                        *scissor,
                        frame,
                    )?;
                }
                Batch::Composite(_)
                | Batch::Shader(_)
                | Batch::Projective(_)
                | Batch::Instant(_) => self.draw_layer(pass, frame, batch, buffers)?,
            }
        }
        Ok(())
    }

    // Draws a layer's batch: its composite, through a runtime shader or
    // tilted, or the instant triangles standing in for it; other batches
    // draw nothing here.
    fn draw_layer(
        &mut self,
        pass: &mut TrackedPass<'_, '_>,
        frame: PassFrame,
        batch: &Batch<'_>,
        buffers: &PassBuffers,
    ) -> Result<(), String> {
        let target_size = frame.size;
        match batch {
            Batch::Composite(prepared) => self.effect_renderer.draw_prepared_composite(
                pass.untracked(),
                (&self.device, target_size),
                prepared,
            ),
            Batch::Shader(prepared) => self.effect_renderer.draw_prepared_shader_src_over(
                pass.untracked(),
                target_size,
                prepared,
            ),
            Batch::Projective(prepared) => self.effect_renderer.draw_prepared_projective_composite(
                pass.untracked(),
                (&self.device, target_size),
                prepared,
            ),
            Batch::Instant(draw) => {
                let context = (&self.viewport_uniforms, &self.frame_stats);
                let laid = buffers.instant.as_ref();
                return self
                    .pipelines
                    .draw_instant(pass, (frame, draw), laid, context);
            }
            _ => {}
        }
        Ok(())
    }

    /// Begins the pass `encode_pass` records into, with a depth buffer when
    /// its opaque interiors go down first.
    fn begin_scene_pass<'p, C: FrameCommandRecorder>(
        &mut self,
        recorder: &'p mut C,
        label: &'static str,
        target: PassTarget<'_>,
        load_op: wgpu::LoadOp<wgpu::Color>,
        depth: PassDepth,
    ) -> wgpu::RenderPass<'p> {
        if depth.is_on() {
            self.frame_stats
                .depth_passes
                .set(self.frame_stats.depth_passes.get() + 1);
            let depth_view = self
                .depth_targets
                .interior(&self.device, (target.width, target.height));
            recorder.begin_depth_pass(label, target.view, load_op, &depth_view)
        } else {
            recorder.begin_color_pass(label, target.view, load_op)
        }
    }

    /// Records one shape batch's stage; other batches draw nothing here.
    /// An arena batch's draws are ranges of `arena`.
    fn draw_shape_batch(
        &self,
        pass: &mut TrackedPass<'_, '_>,
        batch: &Batch<'_>,
        arena: &[RunDrawCall],
        meshes: Option<crate::mesh_fallback::RunMeshes<'_>>,
        frame: PassFrame,
        stage: RunStage,
    ) -> Result<(), String> {
        let meshes = meshes.map(|meshes| (meshes, frame.depth));
        match batch {
            Batch::StoreRun { batch, scissor } => {
                match stage_scissor(frame.scissor(*scissor), batch.clip, stage) {
                    Some(scissor) => {
                        self.draw_store_run(pass, batch, arena, scissor, stage, meshes)
                    }
                    None => Ok(()),
                }
            }
            Batch::Arena {
                chunk,
                uniform_slot,
                paint,
                interiors,
                scissor,
                clip,
            } => {
                let draws = match stage {
                    RunStage::Paint => paint.clone(),
                    RunStage::Interiors => match interiors {
                        Some(interiors) => interiors.clone(),
                        None => return Ok(()),
                    },
                };
                let Some(scissor) = stage_scissor(frame.scissor(*scissor), *clip, stage) else {
                    return Ok(());
                };
                let meshes = meshes.map(|(meshes, depth)| (meshes.of(draws.clone()), depth));
                let calls = RunCalls {
                    uniform_slot: *uniform_slot,
                    draws: &arena[draws],
                    scissor,
                    stage,
                    meshes,
                };
                self.draw_arena(pass, *chunk, calls)
            }
            _ => Ok(()),
        }
    }
}
