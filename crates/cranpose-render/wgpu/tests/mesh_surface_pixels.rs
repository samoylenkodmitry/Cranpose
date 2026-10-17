use std::sync::Arc;

use cranpose_core::remember;
use cranpose_macros::composable;
use cranpose_ui::{
    Color, Modifier, Size,
    space::{Camera, Light, Mat4, Mesh, MeshSurface, Scene3d, Vec3},
    widgets::{Box, BoxSpec},
};

use crate::support;

const FRAME_WIDTH: u32 = 400;
const FRAME_HEIGHT: u32 = 300;
const FOCAL_LENGTH: f32 = 600.0;
const EYE_Z: f32 = -800.0;
const SPHERE: Vec3 = Vec3::new(0.0, 0.0, 200.0);
const SPHERE_RADIUS: f32 = 150.0;
const NEAR_PANEL: Vec3 = Vec3::new(60.0, 0.0, 0.0);
const NEAR_PANEL_SIDE: f32 = 100.0;

fn camera() -> Camera {
    Camera::look_at(
        Vec3::new(0.0, 0.0, EYE_Z),
        Vec3::ZERO,
        Vec3::new(0.0, 1.0, 0.0),
        FOCAL_LENGTH,
    )
}

/// The frame pixel at the scene point `point`.
fn pixel_at(frame: &cranpose_render_wgpu::CapturedFrame, point: Vec3) -> [u8; 4] {
    let shown = camera()
        .project(
            point,
            Size {
                width: FRAME_WIDTH as f32,
                height: FRAME_HEIGHT as f32,
            },
        )
        .expect("in front of the eye");
    let index = (shown.y as usize * frame.width as usize + shown.x as usize) * 4;
    [
        frame.pixels[index],
        frame.pixels[index + 1],
        frame.pixels[index + 2],
        frame.pixels[index + 3],
    ]
}

#[composable]
fn Surface(mesh: Arc<Mesh>, transform: Mat4, (lit, casts_shadow, color): (bool, bool, Color)) {
    Box(
        Modifier::empty()
            .required_size(Size {
                width: 256.0,
                height: 128.0,
            })
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&mesh),
                transform,
                lit,
                casts_shadow,
            })
            .background(color),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn MeshScene() {
    let (plane, sphere) = remember(|| (Arc::new(Mesh::plane()), Arc::new(Mesh::sphere(48, 24))))
        .with(|(plane, sphere)| (Arc::clone(plane), Arc::clone(sphere)));
    Box(
        Modifier::empty()
            .size(Size {
                width: FRAME_WIDTH as f32,
                height: FRAME_HEIGHT as f32,
            })
            .background(Color::BLACK)
            .scene3d(|| Scene3d {
                camera: camera(),
                light: Some(Light {
                    direction: Vec3::new(1.0, 0.0, 1.0),
                    ambient: 0.2,
                    shadows: true,
                }),
            }),
        BoxSpec::default(),
        move || {
            // The near panel comes first: depth, not order, hides the sphere.
            Surface(
                Arc::clone(&plane),
                Mat4::translation(Vec3::new(-0.5, -0.5, 0.0))
                    .then(&Mat4::scaling(Vec3::new(
                        NEAR_PANEL_SIDE,
                        NEAR_PANEL_SIDE,
                        1.0,
                    )))
                    .then(&Mat4::translation(NEAR_PANEL)),
                (false, false, Color(0.0, 1.0, 0.0, 1.0)),
            );
            Surface(
                Arc::clone(&sphere),
                Mat4::scaling(Vec3::new(SPHERE_RADIUS, SPHERE_RADIUS, SPHERE_RADIUS))
                    .then(&Mat4::translation(SPHERE)),
                (true, true, Color(1.0, 0.0, 0.0, 1.0)),
            );
            Surface(
                Arc::clone(&plane),
                Mat4::translation(Vec3::new(-0.5, -0.5, 0.0))
                    .then(&Mat4::scaling(Vec3::new(2000.0, 1500.0, 1.0)))
                    .then(&Mat4::translation(Vec3::new(0.0, 0.0, 600.0))),
                (false, false, Color(0.0, 0.0, 1.0, 1.0)),
            );
        },
    );
}

#[test]
fn mesh_surfaces_draw_their_content_lit_shadowed_and_nearest_first() {
    let Some((_lock, mut shell)) = support::app_shell_for(
        MeshScene,
        FRAME_WIDTH,
        FRAME_HEIGHT,
        wgpu::TextureFormat::Rgba8Unorm,
        |_| {},
    ) else {
        return;
    };
    let (frame, _) = support::warm_shell_frame(&mut shell, FRAME_WIDTH, FRAME_HEIGHT);

    let near_panel = pixel_at(&frame, NEAR_PANEL);
    assert!(
        near_panel[1] > 200 && near_panel[0] < 40,
        "the near panel hides the sphere behind it: {near_panel:?}"
    );

    let lit_side = pixel_at(&frame, SPHERE + Vec3::new(-0.6, 0.0, -0.8) * SPHERE_RADIUS);
    let dark_side = pixel_at(
        &frame,
        SPHERE + Vec3::new(-0.2, 0.75, -0.63) * SPHERE_RADIUS,
    );
    assert!(
        lit_side[0] > 150 && lit_side[1] < 40 && lit_side[2] < 40,
        "the sphere shows its red content where the light falls: {lit_side:?}"
    );
    assert!(
        dark_side[0] + 40 < lit_side[0],
        "the sphere is shaded: {dark_side:?} away from the light, {lit_side:?} facing it"
    );

    let far_wall = pixel_at(&frame, Vec3::new(-300.0, 0.0, 600.0));
    assert!(
        far_wall[2] > 200 && far_wall[0] < 40,
        "the far wall shows beside the sphere, in the light: {far_wall:?}"
    );
    // The light travels along (1, 0, 1): the sphere's shadow falls on the
    // far wall 400 to the right of its centre.
    let shadow = pixel_at(&frame, SPHERE + Vec3::new(400.0, 0.0, 400.0));
    assert!(
        shadow[2] < 100 && shadow[2] > 20,
        "the sphere's shadow darkens the unlit far wall to its ambient: {shadow:?}"
    );
}
