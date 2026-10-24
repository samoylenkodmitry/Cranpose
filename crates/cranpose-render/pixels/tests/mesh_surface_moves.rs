//! A mesh surface that moves alone, update after update, as an animation
//! moves it, draws where the scene's camera shows it after each move.

use std::{cell::Cell, rc::Rc, sync::Arc};

use cranpose_render_common::SceneUpdates;
use cranpose_render_pixels::{Scene, draw_scene};
use cranpose_ui::{
    Color, LayoutEngine, Modifier, Size,
    space::{Camera, Mat4, Material, Mesh, MeshSurface, Scene3d, Vec3},
    widgets::{Box, BoxSpec},
};

const WIDTH: u32 = 400;
const HEIGHT: u32 = 300;
const PANEL: Size = Size {
    width: 100.0,
    height: 100.0,
};
/// The camera stands one focal length in front of the plane z = 0, so the
/// panel there shows at its own size.
const FOCAL_LENGTH: f32 = 600.0;

/// The panel's transform with its middle at `x` on the plane z = 0.
fn panel_at(x: f32) -> Mat4 {
    Mat4::translation(Vec3::new(-0.5, -0.5, 0.0))
        .then(&Mat4::scaling(Vec3::new(PANEL.width, PANEL.height, 1.0)))
        .then(&Mat4::translation(Vec3::new(x, 0.0, 0.0)))
}

#[test]
fn a_surface_that_moves_again_and_again_draws_where_the_camera_shows_it() {
    let center_x = Rc::new(Cell::new(0.0f32));
    let panel_node = Rc::new(Cell::new(None));
    let (x, node) = (Rc::clone(&center_x), Rc::clone(&panel_node));
    let mut composition = cranpose_ui::run_test_composition(move || {
        let (x, node) = (Rc::clone(&x), Rc::clone(&node));
        let plane = Arc::new(Mesh::plane());
        Box(
            Modifier::empty()
                .size(Size {
                    width: WIDTH as f32,
                    height: HEIGHT as f32,
                })
                .scene3d(|| Scene3d {
                    camera: Camera::look_at(
                        Vec3::new(0.0, 0.0, -FOCAL_LENGTH),
                        Vec3::ZERO,
                        Vec3::new(0.0, 1.0, 0.0),
                        FOCAL_LENGTH,
                    ),
                    light: None,
                    eye: None,
                }),
            BoxSpec::default(),
            move || {
                let (x, plane) = (Rc::clone(&x), Arc::clone(&plane));
                let id = Box(
                    Modifier::empty()
                        .required_size(PANEL)
                        .mesh_surface(move || MeshSurface {
                            mesh: Arc::clone(&plane),
                            transform: panel_at(x.get()),
                            material: Material::Unlit,
                            casts_shadow: false,
                            solid: true,
                        }),
                    BoxSpec::default(),
                    || {
                        Box(
                            Modifier::empty()
                                .fill_max_size()
                                .background(Color(1.0, 0.0, 0.0, 1.0)),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
                node.set(Some(id));
            },
        );
    });
    let panel = panel_node.get().expect("the panel node");
    let root = composition.root().expect("a root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier
        .compute_layout(
            root,
            Size {
                width: WIDTH as f32,
                height: HEIGHT as f32,
            },
        )
        .expect("layout");
    let mut scene = Scene::default();
    scene.rebuild_from_applier(&applier, root);
    // Each move changes the panel's layer alone, so the updates after the
    // first may take a shorter way to it.
    let red_at_middle = [-100.0f32, 0.0, 100.0].map(|x| {
        center_x.set(x);
        scene.update_from_applier(
            &applier,
            root,
            SceneUpdates {
                content: &[],
                draws: &[],
                layers: &[panel],
                moved: &[],
            },
            false,
        );
        let mut frame = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
        draw_scene(&mut frame, WIDTH, HEIGHT, &scene);
        let (column, row) = ((WIDTH as f32 * 0.5 + x) as usize, HEIGHT as usize / 2);
        let index = (row * WIDTH as usize + column) * 4;
        frame[index] > 200 && frame[index + 1] < 40
    });
    applier.clear_runtime_handle();
    assert_eq!(
        red_at_middle, [true; 3],
        "the panel draws where the camera shows it after each move"
    );
}
