//! `cranpose_render_wgpu::clear_to_default_background` — the zero-pipeline
//! clear every platform's surface installation uses to close the
//! black-screen gap: nothing is presented until a first real content
//! frame's pipelines finish compiling. The function's signature has
//! nowhere to put a `PassPipeline`, so "no pipeline touched" is a
//! structural guarantee; this test is about the pixels it actually
//! produces.

use std::{sync::mpsc, time::Duration};

use cranpose_render_wgpu::clear_to_default_background;

fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    instance_descriptor.backends = wgpu::Backends::all();
    let instance = wgpu::Instance::new(instance_descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok()?;
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("initial-present-clear-test-device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .ok()
}

/// `clear_to_default_background` must produce exactly
/// `cranpose_render_common::FRAME_CLEAR_COLOR` — the same base every real
/// frame clears to underneath its own content — so a placeholder frame
/// never flashes a different colour than the content that replaces it.
#[test]
fn clears_every_pixel_to_the_frameworks_default_background() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available; skipping placeholder clear pixel test");
        return;
    };

    const WIDTH: u32 = 4;
    const HEIGHT: u32 = 4;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("placeholder-clear-test-target"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    clear_to_default_background(&device, &queue, &view);

    let unpadded_bytes_per_row = WIDTH * 4;
    let padded_bytes_per_row =
        unpadded_bytes_per_row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("placeholder-clear-test-readback"),
        size: (padded_bytes_per_row * HEIGHT) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("placeholder-clear-test-copy"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    let submission = queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: None,
    });
    rx.recv_timeout(Duration::from_secs(3))
        .expect("readback must complete")
        .expect("map_async must succeed");

    let mapped = slice.get_mapped_range();
    let expected: [u8; 4] = [
        (cranpose_render_common::FRAME_CLEAR_COLOR[0] * 255.0).round() as u8,
        (cranpose_render_common::FRAME_CLEAR_COLOR[1] * 255.0).round() as u8,
        (cranpose_render_common::FRAME_CLEAR_COLOR[2] * 255.0).round() as u8,
        (cranpose_render_common::FRAME_CLEAR_COLOR[3] * 255.0).round() as u8,
    ];
    for row in 0..HEIGHT as usize {
        let row_start = row * padded_bytes_per_row as usize;
        for col in 0..WIDTH as usize {
            let pixel_start = row_start + col * 4;
            let pixel = &mapped[pixel_start..pixel_start + 4];
            assert_eq!(
                pixel, expected,
                "pixel ({col}, {row}) must be the framework's default background"
            );
        }
    }
}
