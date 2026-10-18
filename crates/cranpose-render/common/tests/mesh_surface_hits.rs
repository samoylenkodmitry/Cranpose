use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_render_common::{
    HitTestTarget, RenderScene, graph_scene::Scene, scene_builder::build_graph_from_applier,
};
use cranpose_ui::{
    LayoutEngine, Modifier, Point, Size,
    space::{Camera, Mat4, Material, Mesh, MeshSurface, Scene3d, Vec3},
    widgets::{Box, BoxSpec},
};

const VIEWPORT: Size = Size {
    width: 800.0,
    height: 600.0,
};
const PANEL: Size = Size {
    width: 300.0,
    height: 200.0,
};
const FOCAL_LENGTH: f32 = 600.0;

fn camera() -> Camera {
    Camera::look_at(
        Vec3::new(0.0, 0.0, -1000.0),
        Vec3::ZERO,
        Vec3::new(0.0, 1.0, 0.0),
        FOCAL_LENGTH,
    )
}

/// A panel of `PANEL`'s size, centred on `center` and turned `turn`
/// radians about the vertical axis.
fn panel_transform(center: Vec3, turn: f32) -> Mat4 {
    Mat4::translation(Vec3::new(-0.5, -0.5, 0.0))
        .then(&Mat4::scaling(Vec3::new(PANEL.width, PANEL.height, 1.0)))
        .then(&Mat4::rotation_y(turn))
        .then(&Mat4::translation(center))
}

/// Where the content point `point` of a panel placed by `transform` shows.
fn shown(transform: Mat4, point: Point) -> Point {
    let scene = transform.transform_point(Vec3::new(
        point.x / PANEL.width,
        point.y / PANEL.height,
        0.0,
    ));
    camera()
        .project(scene, VIEWPORT)
        .expect("the panel is in front of the eye")
}

const FAR: Vec3 = Vec3::new(0.0, 0.0, 300.0);
const NEAR: Vec3 = Vec3::new(60.0, 20.0, 0.0);

struct Probe {
    scene: Scene,
    far: usize,
    near: usize,
    pressed: Rc<RefCell<Vec<(&'static str, Point)>>>,
}

fn probe() -> Probe {
    let pressed: Rc<RefCell<Vec<(&'static str, Point)>>> = Rc::default();
    let sink = Rc::clone(&pressed);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let plane = Arc::new(Mesh::plane());
        let sink = Rc::clone(&sink);
        let panel = move |name: &'static str, transform: Mat4| {
            let plane = Arc::clone(&plane);
            let sink = Rc::clone(&sink);
            Box(
                Modifier::empty()
                    .required_size(PANEL)
                    .mesh_surface(move || MeshSurface {
                        mesh: Arc::clone(&plane),
                        transform,
                        material: Material::Unlit,
                        casts_shadow: false,
                    })
                    .clickable(move |point| sink.borrow_mut().push((name, point))),
                BoxSpec::default(),
                || {},
            );
        };
        Box(
            Modifier::empty().fill_max_size().scene3d(|| Scene3d {
                camera: camera(),
                light: None,
            }),
            BoxSpec::default(),
            move || {
                panel("far", panel_transform(FAR, 0.0));
                panel("near", panel_transform(NEAR, 0.3));
            },
        );
    });
    let root = composition.root().expect("a root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier.compute_layout(root, VIEWPORT).expect("layout");
    let graph = build_graph_from_applier(&applier, root, 1.0).expect("render graph");
    applier.clear_runtime_handle();
    let mut scene = Scene::default();
    cranpose_render_common::hit_graph::collect_hits_from_graph(
        &graph.root,
        cranpose_render_common::graph::ProjectiveTransform::identity(),
        &mut scene,
        None,
    );
    scene.replace_graph(graph);
    let mut panels = scene
        .hits
        .iter()
        .filter(|hit| hit.local_bounds.width == PANEL.width)
        .map(|hit| hit.node_id);
    let (Some(far), Some(near)) = (panels.next(), panels.next()) else {
        panic!("both panels take pointer input");
    };
    Probe {
        scene,
        far,
        near,
        pressed,
    }
}

fn press(probe: &Probe, at: Point) -> Option<usize> {
    let target = probe.scene.hit_test(at.x, at.y).into_iter().next()?;
    for kind in [PointerEventKind::Down, PointerEventKind::Up] {
        target.dispatch(PointerEvent::new(kind, at, at));
    }
    Some(target.node_id())
}

#[test]
fn a_press_on_a_surface_reaches_its_content_where_it_shows() {
    let probe = probe();
    let content = Point::new(10.0, 30.0);
    let at = shown(panel_transform(FAR, 0.0), content);
    assert_eq!(press(&probe, at), Some(probe.far));
    let pressed = probe.pressed.borrow();
    let (name, local) = pressed[0];
    assert_eq!(name, "far");
    assert!(
        (local.x - content.x).abs() < 0.5 && (local.y - content.y).abs() < 0.5,
        "the press lands on the content at {local:?}, not {content:?}"
    );
}

#[test]
fn the_nearer_surface_takes_a_press_where_both_show() {
    let probe = probe();
    let near_content = Point::new(150.0, 100.0);
    let at = shown(panel_transform(NEAR, 0.3), near_content);
    let ray = camera().ray(at, VIEWPORT).expect("the view inverts");
    let far_surface = MeshSurface {
        mesh: Arc::new(Mesh::plane()),
        transform: panel_transform(FAR, 0.0),
        material: Material::Unlit,
        casts_shadow: false,
    };
    assert!(
        far_surface.cast(ray, PANEL).is_some(),
        "the far panel lies behind the near one there"
    );
    assert_eq!(press(&probe, at), Some(probe.near));
}

#[test]
fn a_press_beside_every_surface_reaches_none() {
    let probe = probe();
    assert_eq!(press(&probe, Point::new(5.0, 5.0)), None);
}

fn camera_at(eye_x: f32) -> Camera {
    Camera::look_at(
        Vec3::new(eye_x, 0.0, -1000.0),
        Vec3::new(eye_x, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        FOCAL_LENGTH,
    )
}

#[test]
fn a_camera_that_moves_moves_where_its_surfaces_take_input() {
    let eye_x = Rc::new(std::cell::Cell::new(0.0f32));
    let scene_node = Rc::new(std::cell::Cell::new(None));
    let (eye, node) = (Rc::clone(&eye_x), Rc::clone(&scene_node));
    let mut composition = cranpose_ui::run_test_composition(move || {
        let eye = Rc::clone(&eye);
        let plane = Arc::new(Mesh::plane());
        let id = Box(
            Modifier::empty().fill_max_size().scene3d(move || Scene3d {
                camera: camera_at(eye.get()),
                light: None,
            }),
            BoxSpec::default(),
            move || {
                let plane = Arc::clone(&plane);
                Box(
                    Modifier::empty()
                        .required_size(PANEL)
                        .mesh_surface(move || MeshSurface {
                            mesh: Arc::clone(&plane),
                            transform: panel_transform(FAR, 0.0),
                            material: Material::Unlit,
                            casts_shadow: false,
                        })
                        .clickable(|_| {}),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
        node.set(Some(id));
    });
    let root = composition.root().expect("a root");
    let scene_id = scene_node.get().expect("the scene node");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier.compute_layout(root, VIEWPORT).expect("layout");
    let mut scene = Scene::default();
    scene.rebuild_from_applier(&applier, root);

    let near_left_edge = Point::new(10.0, 100.0);
    let project = |eye: f32| {
        let point = panel_transform(FAR, 0.0).transform_point(Vec3::new(
            near_left_edge.x / PANEL.width,
            near_left_edge.y / PANEL.height,
            0.0,
        ));
        camera_at(eye)
            .project(point, VIEWPORT)
            .expect("in front of the eye")
    };
    let before = project(0.0);
    assert_eq!(scene.hit_test(before.x, before.y).len(), 1);

    eye_x.set(300.0);
    scene.update_from_applier(
        &applier,
        root,
        cranpose_render_common::SceneUpdates {
            content: &[],
            layers: &[scene_id],
            moved: &[],
        },
        false,
    );
    applier.clear_runtime_handle();
    let after = project(300.0);
    assert_eq!(
        scene.hit_test(after.x, after.y).len(),
        1,
        "the panel takes input where the moved camera shows it"
    );
    assert!(
        scene.hit_test(before.x, before.y).is_empty(),
        "the panel no longer takes input where the camera showed it before"
    );
}

const WALL: Size = Size {
    width: 600.0,
    height: 400.0,
};
const INNER: Size = Size {
    width: 100.0,
    height: 80.0,
};
/// Where the inner panel's top left corner lies on the wall's content, and
/// how far it stands out of the wall, toward the eye.
const INNER_AT: Vec3 = Vec3::new(450.0, 260.0, -200.0);

/// The surfaces pressed, by name, and the content points they were pressed
/// at.
type Presses = Rc<RefCell<Vec<(&'static str, Point)>>>;

/// Makes a node a pressable mesh surface on the unit plane, from its name,
/// its content's size and its transform.
type Pressable = Rc<dyn Fn((&'static str, Size, Mat4)) -> Modifier>;

/// A wall facing the eye, its content's middle at `FAR` and a content pixel
/// to a scene pixel, whose content `inside` composes; each surface records
/// the content points it is pressed at.
fn wall_scene(inside: impl Fn(&Pressable) + 'static) -> (Scene, Presses) {
    let pressed: Presses = Rc::default();
    let sink = Rc::clone(&pressed);
    let inside = Rc::new(inside);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let plane = Arc::new(Mesh::plane());
        let sink = Rc::clone(&sink);
        let surface: Pressable = Rc::new(move |(name, size, transform)| {
            let plane = Arc::clone(&plane);
            let sink = Rc::clone(&sink);
            Modifier::empty()
                .required_size(size)
                .mesh_surface(move || MeshSurface {
                    mesh: Arc::clone(&plane),
                    transform,
                    material: Material::Unlit,
                    casts_shadow: false,
                })
                .clickable(move |point| sink.borrow_mut().push((name, point)))
        });
        let wall = surface((
            "wall",
            WALL,
            Mat4::translation(Vec3::new(-0.5, -0.5, 0.0))
                .then(&Mat4::scaling(Vec3::new(WALL.width, WALL.height, 1.0)))
                .then(&Mat4::translation(FAR)),
        ));
        let inside = Rc::clone(&inside);
        Box(
            Modifier::empty().fill_max_size().scene3d(|| Scene3d {
                camera: camera(),
                light: None,
            }),
            BoxSpec::default(),
            move || {
                let (inside, surface) = (Rc::clone(&inside), Rc::clone(&surface));
                Box(wall.clone(), BoxSpec::default(), move || inside(&surface));
            },
        );
    });
    let root = composition.root().expect("a root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    applier.compute_layout(root, VIEWPORT).expect("layout");
    let mut scene = Scene::default();
    scene.rebuild_from_applier(&applier, root);
    applier.clear_runtime_handle();
    (scene, pressed)
}

/// A wall with an inner panel inside its content that stands out of it,
/// laid out where it would lie flat on the wall.
fn wall_with_inner_panel() -> (Scene, Presses) {
    wall_scene(|surface| {
        let inner = surface((
            "inner",
            INNER,
            Mat4::scaling(Vec3::new(INNER.width, INNER.height, 1.0))
                .then(&Mat4::translation(INNER_AT)),
        ));
        Box(
            Modifier::empty()
                .absolute_offset(INNER_AT.x, INNER_AT.y)
                .then(inner),
            BoxSpec::default(),
            || {},
        );
    })
}

/// Where the eye sees the wall's content point `(x, y)` lifted `z` toward
/// it: the wall's content pixel is a scene pixel, and its middle lies at
/// `FAR`.
fn wall_content_shown(x: f32, y: f32, z: f32) -> Point {
    let scene = Vec3::new(
        FAR.x + x - WALL.width * 0.5,
        FAR.y + y - WALL.height * 0.5,
        FAR.z + z,
    );
    camera()
        .project(scene, VIEWPORT)
        .expect("in front of the eye")
}

fn press_at(scene: &Scene, at: Point) {
    if let Some(target) = scene.hit_test(at.x, at.y).into_iter().next() {
        for kind in [PointerEventKind::Down, PointerEventKind::Up] {
            target.dispatch(PointerEvent::new(kind, at, at));
        }
    }
}

#[test]
fn a_surface_inside_another_takes_a_press_where_it_stands_out() {
    let (scene, pressed) = wall_with_inner_panel();
    let content = Point::new(50.0, 40.0);
    press_at(
        &scene,
        wall_content_shown(INNER_AT.x + content.x, INNER_AT.y + content.y, INNER_AT.z),
    );
    let pressed = pressed.borrow();
    let Some(&(name, local)) = pressed.first() else {
        panic!("the press reached nothing");
    };
    assert_eq!(name, "inner");
    assert!(
        (local.x - content.x).abs() < 0.5 && (local.y - content.y).abs() < 0.5,
        "the press lands on the inner panel's content at {local:?}, not {content:?}"
    );
}

#[test]
fn the_wall_takes_a_press_where_the_inner_surface_would_lie_flat() {
    let (scene, pressed) = wall_with_inner_panel();
    // The inner panel stands out of the wall, so the eye sees it away from
    // the place it would take on the wall's content.
    let on_wall = Point::new(INNER_AT.x + 5.0, INNER_AT.y + 5.0);
    press_at(&scene, wall_content_shown(on_wall.x, on_wall.y, 0.0));
    let pressed = pressed.borrow();
    let Some(&(name, local)) = pressed.first() else {
        panic!("the press reached nothing");
    };
    assert_eq!(name, "wall");
    assert!(
        (local.x - on_wall.x).abs() < 0.5 && (local.y - on_wall.y).abs() < 0.5,
        "the press lands on the wall's content at {local:?}, not {on_wall:?}"
    );
}

/// The window, a rect of the wall's content, its middle at the wall's.
const WINDOW: (f32, f32, f32, f32) = (200.0, 100.0, 200.0, 200.0);
/// The focal length of the scene behind the window, whose camera stands
/// that far in front of its origin, in the doorway, and how far behind the
/// origin its panel stands.
const NESTED: (f32, f32) = (200.0, 100.0);

/// A wall with a window into a nested scene, which holds a pressable panel
/// behind the window.
fn wall_with_window() -> (Scene, Presses) {
    wall_scene(|surface| {
        let panel = surface((
            "nested",
            INNER,
            Mat4::scaling(Vec3::new(INNER.width, INNER.height, 1.0)).then(&Mat4::translation(
                Vec3::new(-INNER.width * 0.5, -INNER.height * 0.5, NESTED.1),
            )),
        ));
        let (x, y, width, height) = WINDOW;
        Box(
            Modifier::empty()
                .absolute_offset(x, y)
                .size(Size { width, height })
                .scene3d(|| Scene3d {
                    camera: Camera::look_at(
                        Vec3::new(0.0, 0.0, -NESTED.0),
                        Vec3::ZERO,
                        Vec3::new(0.0, 1.0, 0.0),
                        NESTED.0,
                    ),
                    light: None,
                }),
            BoxSpec::default(),
            move || {
                Box(panel.clone(), BoxSpec::default(), || {});
            },
        );
    })
}

#[test]
fn a_press_through_a_window_reaches_the_space_behind_it() {
    let (scene, pressed) = wall_with_window();
    // The nested camera stands on the wall, so the panel, its middle on the
    // camera's axis, stands NESTED.0 + NESTED.1 behind the wall's middle.
    let content = Point::new(30.0, 20.0);
    press_at(
        &scene,
        wall_content_shown(
            WALL.width * 0.5 - INNER.width * 0.5 + content.x,
            WALL.height * 0.5 - INNER.height * 0.5 + content.y,
            NESTED.0 + NESTED.1,
        ),
    );
    let pressed = pressed.borrow();
    let Some(&(name, local)) = pressed.first() else {
        panic!("the press reached nothing");
    };
    assert_eq!(name, "nested");
    assert!(
        (local.x - content.x).abs() < 0.5 && (local.y - content.y).abs() < 0.5,
        "the press lands on the panel behind the window at {local:?}, not {content:?}"
    );
}

#[test]
fn a_press_through_a_window_where_its_space_shows_nothing_reaches_the_wall() {
    let (scene, pressed) = wall_with_window();
    let corner = Point::new(WINDOW.0 + 10.0, WINDOW.1 + 10.0);
    press_at(&scene, wall_content_shown(corner.x, corner.y, 0.0));
    let pressed = pressed.borrow();
    let Some(&(name, local)) = pressed.first() else {
        panic!("the press reached nothing");
    };
    assert_eq!(name, "wall");
    assert!(
        (local.x - corner.x).abs() < 0.5 && (local.y - corner.y).abs() < 0.5,
        "the press lands on the wall's content at {local:?}, not {corner:?}"
    );
}
