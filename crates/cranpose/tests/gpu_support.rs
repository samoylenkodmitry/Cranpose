use cranpose::GpuContext;

pub(crate) fn fill(gpu: &GpuContext, texture: &wgpu::Texture, color: wgpu::Color) {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: None,
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(color),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    gpu.queue.submit([encoder.finish()]);
}

pub(crate) fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> &[u8] {
    let start = ((y * width + x) * 4) as usize;
    &pixels[start..start + 4]
}
