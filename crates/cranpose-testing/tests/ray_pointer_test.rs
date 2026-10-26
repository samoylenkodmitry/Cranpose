//! A pointer that points along a ray in a 3D scene, as a controller, a hand
//! or a gaze does, from somewhere other than the window's eye, presses the
//! surface content the ray meets, where it meets it.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_app_shell::AppShell;
use cranpose_core::{NodeId, location_key};
use cranpose_foundation::PointerId;
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    Box, BoxSpec, Modifier, Point, PointerEventKind, Size,
    space::{Camera, Mat4, Material, Mesh, MeshSurface, Ray, Scene3d, Vec3},
};

const PANEL: Size = Size {
    width: 200.0,
    height: 100.0,
};
/// Where the panel's top left corner stands in the scene.
const PANEL_AT: Vec3 = Vec3::new(60.0, 50.0, 0.0);

type Clicks = Rc<RefCell<Vec<Point>>>;

/// A shell showing a scene with a clickable panel, the scene's node, and the
/// panel's clicks.
fn panel_shell() -> (AppShell<TestRenderer>, NodeId, Clicks) {
    let clicks: Clicks = Rc::default();
    let scene_node = Rc::new(std::cell::Cell::new(None));
    let (sink, node) = (Rc::clone(&clicks), Rc::clone(&scene_node));
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let sink = Rc::clone(&sink);
            let id = Box(
                Modifier::empty().fill_max_size().scene3d(|| Scene3d {
                    camera: Camera::look_at(
                        Vec3::new(160.0, 100.0, -600.0),
                        Vec3::new(160.0, 100.0, 0.0),
                        Vec3::new(0.0, 1.0, 0.0),
                        600.0,
                    ),
                    light: None,
                    eye: None,
                }),
                BoxSpec::default(),
                move || {
                    let sink = Rc::clone(&sink);
                    let plane = Arc::new(Mesh::plane());
                    Box(
                        Modifier::empty()
                            .required_size(PANEL)
                            .mesh_surface(move || MeshSurface {
                                mesh: Arc::clone(&plane),
                                transform: Mat4::scaling(Vec3::new(PANEL.width, PANEL.height, 1.0))
                                    .then(&Mat4::translation(PANEL_AT)),
                                material: Material::Unlit,
                                casts_shadow: false,
                                solid: true,
                            })
                            .clickable(move |point| sink.borrow_mut().push(point)),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            node.set(Some(id));
        },
    );
    shell.set_viewport(320.0, 200.0);
    shell.set_buffer_size(320, 200);
    shell.update();
    let scene = scene_node.get().expect("the scene composed");
    (shell, scene, clicks)
}

/// The ray from `origin` to the panel's content point `content`.
fn ray_to(origin: Vec3, content: Point) -> Ray {
    let target = PANEL_AT + Vec3::new(content.x, content.y, 0.0);
    Ray {
        origin,
        direction: target - origin,
    }
}

#[test]
fn a_ray_pointer_clicks_the_surface_content_it_meets() {
    let (mut shell, scene, clicks) = panel_shell();
    // A controller below and to the left of the eye.
    let content = Point::new(150.0, 25.0);
    let ray = ray_to(Vec3::new(0.0, 300.0, -400.0), content);
    assert!(shell.ray_pointer((PointerId(1), scene), ray, PointerEventKind::Down));
    assert!(shell.ray_pointer((PointerId(1), scene), ray, PointerEventKind::Up));
    let clicks = clicks.borrow();
    let [at] = clicks.as_slice() else {
        panic!("the ray clicked {} times, not once", clicks.len());
    };
    assert!(
        (at.x - content.x).abs() < 0.5 && (at.y - content.y).abs() < 0.5,
        "the click lands where the ray meets the panel, {at:?}, not {content:?}"
    );
}

#[test]
fn a_second_ray_pointer_passes_by_while_the_first_holds_the_press() {
    let (mut shell, scene, clicks) = panel_shell();
    let left = ray_to(Vec3::new(-200.0, 100.0, -400.0), Point::new(50.0, 50.0));
    let right = ray_to(Vec3::new(500.0, 100.0, -400.0), Point::new(150.0, 50.0));
    shell.ray_pointer((PointerId(1), scene), left, PointerEventKind::Down);
    shell.ray_pointer((PointerId(2), scene), right, PointerEventKind::Down);
    shell.ray_pointer((PointerId(2), scene), right, PointerEventKind::Up);
    let while_held = clicks.borrow().len();
    shell.ray_pointer((PointerId(1), scene), left, PointerEventKind::Up);
    assert_eq!(
        while_held, 0,
        "the second controller's press does not click while the first holds the panel"
    );
    assert_eq!(
        clicks.borrow().len(),
        1,
        "the first controller's release clicks once"
    );
}
