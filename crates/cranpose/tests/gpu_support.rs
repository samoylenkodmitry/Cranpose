use cranpose::{GpuContext, embedded_view::EmbeddedView};

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

/// Draws `view` as a host that honors its frame schedule does: again while
/// the shell asks for a frame, as it does after a frame that left out a draw
/// whose pipeline was still compiling. `pixels` then holds what the view
/// shows once those landed. Returns whether the first draw drew.
pub(crate) fn draw_settled(view: &mut EmbeddedView, pixels: &mut Vec<u8>) -> bool {
    let drew = view.draw(pixels).expect("frame");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while view.shell().needs_redraw() {
        assert!(
            std::time::Instant::now() < deadline,
            "the view kept asking for frames"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
        view.draw(pixels).expect("frame");
    }
    drew
}
