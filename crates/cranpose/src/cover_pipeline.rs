// The render pipeline of a pass that covers its target with one triangle. The shader's vertex
// function is `cover`, and `fragment` names its fragment function. The pass binds
// `bind_group_layout` as group 0 and draws three vertices into a target of `format`.
pub(crate) fn cover_pipeline(
    device: &wgpu::Device,
    label: &str,
    bind_group_layout: &wgpu::BindGroupLayout,
    module: &wgpu::ShaderModule,
    fragment: &str,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(bind_group_layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("cover"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(format.into())],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}
