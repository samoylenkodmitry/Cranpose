mod support;

use cranpose_render_wgpu::{clear_to_default_background, offscreen_render_target_for_tests};
use support::read_texture;

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

#[test]
fn clears_every_pixel_to_the_frameworks_default_background() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter available; skipping placeholder clear pixel test");
        return;
    };

    const WIDTH: u32 = 4;
    const HEIGHT: u32 = 4;
    let (texture, view) =
        offscreen_render_target_for_tests(&device, WIDTH, HEIGHT, "placeholder-clear-test-target");

    clear_to_default_background(&device, &queue, &view);

    let pixels = read_texture(&device, &queue, &texture);

    let expected: [u8; 4] = [
        (cranpose_render_common::FRAME_CLEAR_COLOR[0] * 255.0).round() as u8,
        (cranpose_render_common::FRAME_CLEAR_COLOR[1] * 255.0).round() as u8,
        (cranpose_render_common::FRAME_CLEAR_COLOR[2] * 255.0).round() as u8,
        (cranpose_render_common::FRAME_CLEAR_COLOR[3] * 255.0).round() as u8,
    ];
    for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        let (col, row) = (index % WIDTH as usize, index / WIDTH as usize);
        assert_eq!(
            *pixel, expected,
            "pixel ({col}, {row}) must be the framework's default background"
        );
    }
}
