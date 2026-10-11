use crate::{
    frame_graph::FrameCommandRecorder, lazy_resource::LazyGpuResource,
    pipeline_recorder::PipelineRecorder, shared_shader::SharedShader,
};

pub(crate) struct OutputConverter {
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: LazyGpuResource<wgpu::RenderPipeline>,
    shader: SharedShader,
    format: wgpu::TextureFormat,
}

impl OutputConverter {
    pub(crate) fn new(
        device: &wgpu::Device,
        backend: wgpu::Backend,
        format: wgpu::TextureFormat,
    ) -> Self {
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
        let shader = SharedShader::new(
            device,
            backend,
            "Output Conversion Shader",
            crate::shaders::OUTPUT_CONVERSION,
            &[Some(&bind_group_layout)],
        );
        Self {
            bind_group_layout,
            pipeline: LazyGpuResource::new("output-conversion"),
            shader,
            format,
        }
    }

    fn pipeline(
        &self,
        device: &wgpu::Device,
        backend: wgpu::Backend,
        recorder: &PipelineRecorder,
    ) -> &wgpu::RenderPipeline {
        self.pipeline.for_draw(recorder, backend, || {
            let module = self.shader.module();
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Output Conversion Pipeline"),
                layout: Some(self.shader.layout()),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module,
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
        pipeline_recorder: &PipelineRecorder,
    ) {
        let mut pass = recorder.begin_color_pass(
            "Output Conversion Pass",
            destination,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        );
        pass.set_pipeline(self.pipeline(device, backend, pipeline_recorder));
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}
