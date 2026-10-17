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
