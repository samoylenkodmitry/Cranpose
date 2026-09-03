mod support;

use std::{sync::mpsc, time::Duration};

use support::page::*;

const FRAME_WIDTH: u32 = 320;
const FRAME_HEIGHT: u32 = 240;
const NO_DIRECT: &str = "CRANPOSE_NO_DIRECT_SURFACE";

#[composable]
#[allow(non_snake_case)]
fn CardsPage() {
    FramePage(
        FRAME_WIDTH,
        FRAME_HEIGHT,
        Color(0.05, 0.06, 0.14, 1.0),
        || {
            for i in 0..10u32 {
                let x = (i as f32 * 67.0) % 300.0;
                let y = (i as f32 * 41.0) % 220.0;
                Box(
                    rect_modifier([x, y, 22.0, 22.0])
                        .background(Color(0.9, 0.5 + (i % 4) as f32 * 0.1, 0.3, 1.0))
                        .rounded_corners(11.0),
                    BoxSpec::new(),
                    || {},
                );
            }
            Box(
                rect_modifier([24.0, 40.0, 272.0, 90.0])
                    .backdrop_effect(RenderEffect::blur(7.0))
                    .background(Color(1.0, 1.0, 1.0, 0.18))
                    .rounded_corners(16.0),
                BoxSpec::new(),
                || {
                    Text(
                        "Direct surface",
                        Modifier::empty().offset(16.0, 16.0),
                        TextStyle::default(),
                    );
                },
            );
        },
    );
}

struct Frames {
    captured: Vec<u8>,
    presented: Vec<u8>,
    presented_passes: u32,
}

fn read_texture(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    let width = texture.width();
    let height = texture.height();
    let unpadded = width * 4;
    let padded =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("direct surface readback"),
        size: u64::from(padded) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    let submission = queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (tx, rx) = mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = tx.send(result);
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: Some(submission),
        timeout: None,
    });
    rx.recv_timeout(Duration::from_secs(3))
        .expect("readback timed out")
        .expect("readback map failed");
    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((unpadded * height) as usize);
    for row in 0..height as usize {
        let start = row * padded as usize;
        pixels.extend_from_slice(&mapped[start..start + unpadded as usize]);
    }
    pixels
}

fn render_frames(disable_direct: bool) -> Option<Frames> {
    cranpose_render_wgpu::set_debug_toggle("CRANPOSE_COMPOSITION_8BIT", Some("1"));
    cranpose_render_wgpu::set_debug_toggle(NO_DIRECT, disable_direct.then_some("1"));
    let frames = render_frames_with_current_toggles();
    cranpose_render_wgpu::set_debug_toggle(NO_DIRECT, None);
    frames
}

fn render_frames_with_current_toggles() -> Option<Frames> {
    let (_lock, mut shell) = support::app_shell_for(
        CardsPage,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Rgba8Unorm,
        |_| {},
    )?;
    shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("warm-up capture should succeed");
    let captured = shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("capture should succeed");

    let device = shell.renderer().try_device().expect("device").clone();
    let queue = shell
        .renderer()
        .try_queue_for_tests()
        .expect("queue")
        .clone();
    let (texture, view) = support::render_target(
        &device,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
    );
    for _ in 0..2 {
        shell
            .renderer()
            .render(&texture, &view, FRAME_WIDTH, FRAME_HEIGHT)
            .expect("presentable render should succeed");
    }
    let presented_passes = shell
        .renderer()
        .last_frame_stats()
        .expect("stats")
        .pass_count;
    assert_eq!(shell.renderer().device_error_count_for_tests(), 0);
    Some(Frames {
        captured: captured.pixels,
        presented: read_texture(&device, &queue, &texture),
        presented_passes,
    })
}

#[test]
fn a_frame_rendered_straight_into_the_presentable_image_matches_the_converted_capture() {
    let Some(frames) = render_frames(false) else {
        return;
    };
    assert!(
        frames
            .captured
            .as_chunks::<4>()
            .0
            .iter()
            .any(|px| px[0] > 200 && px[2] < 120),
        "the scene must show its orange dots"
    );
    support::assert_same_bytes(
        "direct vs captured",
        FRAME_WIDTH,
        &frames.presented,
        &frames.captured,
    );

    let kill_switched = render_frames(true).expect("headless WGPU init failed mid-suite");
    support::assert_same_bytes(
        "composition vs captured",
        FRAME_WIDTH,
        &kill_switched.presented,
        &kill_switched.captured,
    );
    assert_eq!(
        frames.presented_passes + 1,
        kill_switched.presented_passes,
        "rendering into the presentable image must drop exactly the output conversion pass"
    );
}
