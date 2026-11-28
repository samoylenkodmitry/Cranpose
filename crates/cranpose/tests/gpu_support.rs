use cranpose::{GpuContext, GuestView, embedded_view::EmbeddedView};

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
    draw_until_settled(view, |view| {
        let more = view.shell().needs_redraw();
        if more {
            view.draw(pixels).expect("frame");
        }
        more
    });
    drew
}

// A view that `draw_until_settled` draws, which tells what its shell and its
// renderer say while it keeps asking for frames.
pub(crate) trait Settling {
    fn state(&mut self) -> String;
}

// The shell's own reasons for a frame and the last frame's counters, which
// count the draws that waited for a pipeline and the cache misses that ask
// for another frame. The renderer asks for a frame of its own while a
// stand-in waits for a pipeline to land, and says whether a compile is still
// in flight for it.
macro_rules! shell_state {
    ($shell:expr) => {{
        use cranpose_render_common::Renderer as _;
        let shell = $shell;
        format!(
            "needs update {}, needs update without a frame {}, pending UI {}, animations {}, \
             frame callbacks {}, renderer asks for a frame {}, renderer awaits a compile {}, \
             last frame {:?}",
            shell.needs_update(),
            shell.needs_update_without_frame(),
            shell.has_pending_ui(),
            shell.has_active_animations(),
            shell.has_transient_frame_callbacks(),
            shell.renderer().needs_frame_warmup(),
            shell.renderer().awaits_pipelines(),
            shell.renderer().last_frame_stats(),
        )
    }};
}

impl Settling for EmbeddedView {
    fn state(&mut self) -> String {
        shell_state!(self.shell())
    }
}

impl Settling for GuestView {
    fn state(&mut self) -> String {
        shell_state!(self.shell())
    }
}

// Calls `frame` on `view`, which draws when the view asks for a frame and
// says whether it did, until it asks for none, a frame at a time as a
// display asks for them. A loop that draws again at once submits a frame
// every few microseconds; on a busy machine it starves the off-frame
// compile of the pipelines those frames wait for, past the 30 s this
// allows. A view that still asks then fails with what it says of itself.
pub(crate) fn draw_until_settled<V: Settling>(view: &mut V, mut frame: impl FnMut(&mut V) -> bool) {
    let started = std::time::Instant::now();
    let deadline = started + std::time::Duration::from_secs(30);
    let mut frames = 0_u32;
    while frame(view) {
        frames += 1;
        assert!(
            std::time::Instant::now() < deadline,
            "the view kept asking for frames: {frames} frames in {:?}; {}",
            started.elapsed(),
            view.state()
        );
        std::thread::sleep(FRAME_PAUSE);
    }
}

// The pause between the frames of `draw_until_settled`.
const FRAME_PAUSE: std::time::Duration = std::time::Duration::from_millis(2);

// The pixels of `texture`, four bytes each, as the GPU holds them.
pub(crate) fn read_texture(gpu: &GpuContext, texture: &wgpu::Texture) -> Vec<u8> {
    let (width, height) = (texture.width(), texture.height());
    let row = (width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| result.expect("map"));
    gpu.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("poll");
    let mapped = slice.get_mapped_range().expect("mapped range");
    let bytes: &[u8] = &mapped;
    bytes
        .chunks_exact(row as usize)
        .flat_map(|line| &line[..(width * 4) as usize])
        .copied()
        .collect()
}
