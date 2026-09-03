use crate::{frame_graph::FrameCommandRecorder, lazy_resource::LazyGpuResource};

const OUTPUT_CONVERSION_SHADER: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );
    return VertexOutput(
        vec4<f32>(positions[index], 0.0, 1.0),
        uvs[index],
    );
}

@group(0) @binding(0) var source_texture: texture_2d<f32>;

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(source_texture));
    let position = clamp(vec2<i32>(floor(input.uv * size)), vec2<i32>(0), vec2<i32>(size) - vec2<i32>(1));
    let color = clamp(textureLoad(source_texture, position, 0), vec4<f32>(0.0), vec4<f32>(1.0));
    return floor(color * 255.0 + vec4<f32>(0.5)) / 255.0;
}
"#;

pub(crate) struct OutputConverter {
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    format: wgpu::TextureFormat,
}

impl OutputConverter {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Output Conversion Bind Group Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Output Conversion Shader"),
            source: wgpu::ShaderSource::Wgsl(OUTPUT_CONVERSION_SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Output Conversion Pipeline Layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        Self {
            bind_group_layout,
            pipeline: LazyGpuResource::new("output-conversion"),
            shader,
            pipeline_layout,
            format,
        }
    }

    fn pipeline(&self, device: &wgpu::Device, backend: wgpu::Backend) -> &wgpu::RenderPipeline {
        self.pipeline.get_or_init(backend, || {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Output Conversion Pipeline"),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: self.format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        })
    }

    pub(crate) fn bind_group(
        &self,
        device: &wgpu::Device,
        source: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Output Conversion Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(source),
            }],
        })
    }

    pub(crate) fn encode<C: FrameCommandRecorder>(
        &self,
        device: &wgpu::Device,
        recorder: &mut C,
        destination: &wgpu::TextureView,
        bind_group: &wgpu::BindGroup,
        backend: wgpu::Backend,
    ) {
        let mut pass = recorder.begin_color_pass(
            "Output Conversion Pass",
            destination,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );
        pass.set_pipeline(self.pipeline(device, backend));
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
