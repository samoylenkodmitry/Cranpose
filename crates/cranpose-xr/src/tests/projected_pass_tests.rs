// The pictures that the projection layer draws in place of layers of their own, on the GPU of the
// machine: over the scene of the volumes and against its depth, flat or curved.

use super::*;
use crate::{
    handle::HandleKind,
    projected_pass::{EyeTarget, Picture, ProjectedLayer, ProjectedPass},
};

// The side of the atlas in pixels, which the test fills with one color
const ATLAS: u32 = 8;
const BLUE: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const RED: [u8; 4] = [255, 0, 0, 255];

fn solid(gpu: &GpuContext, side: u32, color: [u8; 4]) -> wgpu::Texture {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("atlas"),
        size: wgpu::Extent3d {
            width: side,
            height: side,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COLOR,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &color.repeat((side * side) as usize),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(side * 4),
            rows_per_image: Some(side),
        },
        texture.size(),
    );
    texture
}

// A blue panel of `width` by `height` meters, `distance` meters ahead of the head.
fn panel(distance: f32, [width, height]: [f32; 2], radius: Option<f32>) -> ProjectedLayer {
    ProjectedLayer {
        picture: Picture::Atlas([0, 0, ATLAS, ATLAS]),
        pose: Pose::at([0.0, 0.0, -distance]),
        size: [width, height],
        radius,
    }
}

// Draws `layers` into `image`, the picture of the eye at the head's middle. `drawn` says whether
// the image holds a picture of the scene.
fn project(gpu: &GpuContext, layers: &[ProjectedLayer], image: &Eye, drawn: bool) {
    let mut pass = ProjectedPass::new(gpu, COLOR, Some(SCENE_DEPTH_FORMAT), |_| {
        (4, GREEN.repeat(16))
    });
    let atlas = solid(gpu, ATLAS, BLUE);
    pass.prepare(gpu, layers, Some(&atlas));
    let camera = EyeCamera::of(&eye(0.0), (EYE, EYE), FirstRow::Top).expect("a camera");
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let target = EyeTarget {
        color: &image.color_view,
        depth: Some(&image.depth_view),
        size: (EYE, EYE),
        drawn,
    };
    pass.draw(gpu, &mut encoder, (0, &camera), &target, layers);
    gpu.queue.submit([encoder.finish()]);
}

fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 4] {
    pixels.as_chunks::<4>().0[(y * EYE + x) as usize]
}

#[test]
fn a_cleared_image_shows_a_panel_where_the_camera_shows_it_at_its_depth() {
    let gpu = gpu();
    let image = Eye::new(&gpu);
    project(&gpu, &[panel(1.0, [0.5, 0.5], None)], &image, false);
    let pixels = read(&gpu, &image.color, 4);
    // A view of 90 degrees each way puts 64 pixels on a tangent of one, so the panel's half side
    // of 0.25 m a meter ahead covers 16 pixels
    assert_eq!(drawn_box(&pixels), Some([48, 48, 79, 79]));
    assert_eq!(pixel(&pixels, EYE / 2, EYE / 2), BLUE);
    let near = EyeCamera::of(&eye(0.0), (EYE, EYE), FirstRow::Top)
        .expect("a camera")
        .camera
        .near();
    let depth = read_depth(&gpu, &image.depth);
    let at = |x: u32, y: u32| depth[(y * EYE + x) as usize];
    let expected = 1.0 - near / 1000.0;
    assert!(
        (at(EYE / 2, EYE / 2) - expected).abs() < 1e-4,
        "{}",
        at(64, 64)
    );
    assert!((at(2, 2) - 1.0).abs() < 1e-6, "the pass clears the depth");
}

#[test]
fn a_panel_covers_the_volume_behind_it_and_hides_behind_the_volume_in_front() {
    let mut headset = headset();
    let mut scene = VolumePass::default();
    let gpu = headset.gpu.clone();
    // The volume's red quad stands a meter ahead and spans 0.6 m
    let front = Eye::new(&gpu);
    assert!(draw(&mut headset, &mut scene, &eye(0.0), &front));
    project(&gpu, &[panel(0.8, [0.2, 0.2], None)], &front, true);
    let pixels = read(&gpu, &front.color, 4);
    assert_eq!(
        pixel(&pixels, EYE / 2, EYE / 2),
        BLUE,
        "a nearer panel covers"
    );
    assert_eq!(
        pixel(&pixels, 50, EYE / 2),
        RED,
        "the scene stays around it"
    );

    let behind = Eye::new(&gpu);
    assert!(draw(&mut headset, &mut scene, &eye(0.0), &behind));
    project(&gpu, &[panel(1.5, [1.2, 1.2], None)], &behind, true);
    let pixels = read(&gpu, &behind.color, 4);
    assert_eq!(
        pixel(&pixels, EYE / 2, EYE / 2),
        RED,
        "a nearer volume hides"
    );
    assert_eq!(
        pixel(&pixels, 40, EYE / 2),
        BLUE,
        "the panel shows beside the volume"
    );
}

#[test]
fn a_curved_panel_bends_toward_the_viewer_as_a_cylinder_layer_does() {
    let gpu = gpu();
    let width = |radius| {
        let image = Eye::new(&gpu);
        project(&gpu, &[panel(1.0, [1.0, 0.2], radius)], &image, false);
        let [left, _, right, _] = drawn_box(&read(&gpu, &image.color, 4)).expect("a panel");
        right - left
    };
    // The flat panel's edge lies at a tangent of 0.5. On a cylinder of 1 m about the eye, the edge
    // lies 0.5 rad around it, at a tangent of 0.546, so the curved panel shows 6 pixels wider.
    let (flat, curved) = (width(None), width(Some(1.0)));
    assert!(
        (5..=7).contains(&(curved - flat)),
        "a flat panel {flat} pixels wide and a curved one {curved}"
    );
}

#[test]
fn a_handle_shows_the_picture_of_its_sphere() {
    let gpu = gpu();
    let image = Eye::new(&gpu);
    let handle = ProjectedLayer {
        picture: Picture::Handle(HandleKind::Move),
        ..panel(1.0, [0.1, 0.1], None)
    };
    project(&gpu, &[panel(1.2, [0.5, 0.5], None), handle], &image, false);
    let pixels = read(&gpu, &image.color, 4);
    assert_eq!(pixel(&pixels, EYE / 2, EYE / 2), GREEN, "the nearer handle");
    assert_eq!(pixel(&pixels, 54, EYE / 2), BLUE, "the panel behind it");
}
