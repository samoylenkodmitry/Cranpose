use std::sync::atomic::AtomicBool;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::Ordering;

use cranpose_ui_graphics::{ShapeRecordBody, ShapeRecordCurve};
use smallvec::SmallVec;

use crate::{
    lazy_resource::LazyGpuResource,
    pipeline_compiler::{CompileLane, PipelineCompiler},
    render::build_pipeline_logged,
    shared_shader::SharedShader,
};

const WORKGROUP: u32 = 64;
const BODY_ROW: u64 = size_of::<ShapeRecordBody>() as u64;
const CURVE_ROW: u64 = size_of::<ShapeRecordCurve>() as u64;

const _: () = assert!(BODY_ROW == 64 && std::mem::offset_of!(ShapeRecordBody, flags) == 36);
const _: () = assert!(CURVE_ROW == 32);

pub(crate) const FIRST_SCREEN_KEY: u64 = u64::MAX;

fn window_rows(limits: &wgpu::Limits) -> u64 {
    let alignment_rows = u64::from(limits.min_storage_buffer_offset_alignment)
        .div_ceil(CURVE_ROW)
        .max(1);
    let fit = (limits.max_storage_buffer_binding_size / BODY_ROW)
        .min(u64::from(limits.max_compute_workgroups_per_dimension) * u64::from(WORKGROUP))
        .min(u64::from(u32::MAX));
    fit / alignment_rows * alignment_rows
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn fits(limits: &wgpu::Limits) -> bool {
    limits.max_compute_workgroup_size_x >= WORKGROUP
        && limits.max_compute_invocations_per_workgroup >= WORKGROUP
        && window_rows(limits) > 0
}

pub(crate) struct ArcTrigFill {
    builder: FillBuilder,
    pipeline: LazyGpuResource<wgpu::ComputePipeline>,
    bind_group_layout: wgpu::BindGroupLayout,
    window_rows: u64,
    #[cfg_attr(target_arch = "wasm32", expect(dead_code))]
    used: AtomicBool,
}

#[derive(Clone)]
struct FillBuilder {
    device: wgpu::Device,
    backend: wgpu::Backend,
    cache: Option<wgpu::PipelineCache>,
    shader: SharedShader,
    layout: wgpu::PipelineLayout,
}

impl FillBuilder {
    fn build(&self) -> wgpu::ComputePipeline {
        build_pipeline_logged("arc trig fill", || {
            self.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("Arc Trig Fill Pipeline"),
                    layout: Some(&self.layout),
                    module: self.shader.module(),
                    entry_point: Some("cs_arc_trig"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    cache: self.cache.as_ref(),
                })
        })
    }
}

pub(crate) struct TrigBindings {
    windows: SmallVec<[wgpu::BindGroup; 1]>,
}

impl ArcTrigFill {
    pub(crate) fn new(
        device: &wgpu::Device,
        backend: wgpu::Backend,
        cache: Option<wgpu::PipelineCache>,
        shader: SharedShader,
    ) -> Self {
        let table = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Arc Trig Fill Bind Group Layout"),
            entries: &[table(1, true), table(2, false)],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Arc Trig Fill Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        Self {
            builder: FillBuilder {
                device: device.clone(),
                backend,
                cache,
                shader,
                layout,
            },
            pipeline: LazyGpuResource::new("Arc Trig Fill Pipeline"),
            bind_group_layout,
            window_rows: window_rows(&device.limits()),
            used: AtomicBool::new(false),
        }
    }

    pub(crate) fn warm(&self, compiler: &PipelineCompiler) {
        let builder = self.builder.clone();
        let pipeline = self.pipeline.clone();
        compiler.enqueue(CompileLane::WarmUp, move || {
            pipeline.get_or_init(builder.backend, || builder.build());
        });
    }

    fn pipeline(&self) -> &wgpu::ComputePipeline {
        self.pipeline
            .get_or_init(self.builder.backend, || self.builder.build())
    }

    pub(crate) fn bind(&self, bodies: &wgpu::Buffer, curves: &wgpu::Buffer) -> TrigBindings {
        let rows = (bodies.size() / BODY_ROW).min(curves.size() / CURVE_ROW);
        let windows = (0..rows.div_ceil(self.window_rows))
            .map(|index| {
                let first = index * self.window_rows;
                let count = self.window_rows.min(rows - first);
                let window = |buffer, row| {
                    wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer,
                        offset: first * row,
                        size: wgpu::BufferSize::new(count * row),
                    })
                };
                self.builder
                    .device
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("Arc Trig Fill Bind Group"),
                        layout: &self.bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: window(bodies, BODY_ROW),
                            },
                            wgpu::BindGroupEntry {
                                binding: 2,
                                resource: window(curves, CURVE_ROW),
                            },
                        ],
                    })
            })
            .collect();
        TrigBindings { windows }
    }

    pub(crate) fn dispatch(
        &self,
        pass: &mut wgpu::ComputePass<'_>,
        bindings: &TrigBindings,
        rows: u32,
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        if !self.used.swap(true, Ordering::Relaxed) {
            crate::pipeline_disk_cache::note_first_screen_pipeline(FIRST_SCREEN_KEY);
        }
        pass.set_pipeline(self.pipeline());
        let mut remaining = u64::from(rows);
        for window in &bindings.windows {
            if remaining == 0 {
                break;
            }
            let count = remaining.min(self.window_rows);
            pass.set_bind_group(0, window, &[]);
            pass.dispatch_workgroups(
                u32::try_from(count.div_ceil(u64::from(WORKGROUP)))
                    .expect("a window's workgroups fit a dispatch"),
                1,
                1,
            );
            remaining -= count;
        }
    }
}
