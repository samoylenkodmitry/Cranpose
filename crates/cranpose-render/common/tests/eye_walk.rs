//! A window's eye among the spaces of its scene: it walks through doorways
//! and not through walls, sees through doorways as deep as they show on
//! screen and no deeper than they show, and pointers that point along any
//! ray reach the surfaces behind doorways.

use std::{cell::Cell, rc::Rc};

use cranpose_core::NodeId;
use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_render_common::{HitTestTarget, RenderScene};
use cranpose_ui::{
    Modifier, Point,
    space::{Camera, Mat4, Ray, Scene3d, Vec3},
    widgets::{Box, BoxSpec},
};

use crate::mesh_surface_hits::{
    EYE_RADIUS, FAR, FOCAL_LENGTH, INNER, NESTED, Pressable, Presses, WALL, WalkRoom,
    camera_looking, camera_moved, nested_window, press_middle, pressable, walk_room,
    window_into_nested, with_scene,
};

#[test]
fn a_wall_without_a_doorway_stops_the_eye() {
    // Beside the doorway, which spans x and y from -100 to 100 on the wall.
    let WalkRoom {
        mut composition,
        eye,
        ..
    } = walk_room(Vec3::new(250.0, 150.0, 0.0));
    with_scene(&mut composition, |_, _, _| {
        eye.walk(Vec3::new(0.0, 0.0, 1000.0));
    });
    let pose = eye.pose();
    assert!(
        pose.doorways.is_empty(),
        "the eye stays in the room: {pose:?}"
    );
    let gap = FAR.z - pose.position.z;
    assert!(
        (EYE_RADIUS - 0.5..=EYE_RADIUS + 0.5).contains(&gap),
        "the eye stops its radius before the wall, {gap} from it"
    );
}

#[test]
fn an_eye_that_walks_back_out_of_a_doorway_stands_in_the_room_again() {
    let WalkRoom {
        mut composition,
        eye,
        scene,
        nested,
        ..
    } = walk_room(Vec3::new(0.0, 0.0, 0.0));
    let (inside, outside) = with_scene(&mut composition, |mut view, applier, root| {
        eye.walk(Vec3::new(0.0, 0.0, FAR.z + 150.0));
        camera_moved(&mut view, applier, (root, scene));
        let inside = eye.pose();
        eye.walk(Vec3::new(0.0, 0.0, -300.0));
        (inside, eye.pose())
    });
    assert_eq!(&*inside.doorways, &[nested]);
    assert!(
        outside.doorways.is_empty(),
        "walking back through the doorway leaves the space behind it: {outside:?}"
    );
    assert!(
        (outside.position.z - (FAR.z - 150.0)).abs() < 0.5,
        "the eye stands where it walked to, in the room's coordinates: {outside:?}"
    );
}

/// `depth` rooms, each behind a doorway in the middle of the wall of the
/// one before, and a panel in the last; every room is the room of
/// [`walk_room`]'s nested scene.
fn rooms(surface: &Pressable, depth: usize) {
    let surface = Rc::clone(surface);
    Box(nested_window(), BoxSpec::default(), move || {
        if depth == 0 {
            let panel = surface((
                "deep",
                INNER,
                Mat4::scaling(Vec3::new(INNER.width, INNER.height, 1.0)).then(&Mat4::translation(
                    Vec3::new(-INNER.width * 0.5, -INNER.height * 0.5, NESTED.1),
                )),
            ));
            Box(panel, BoxSpec::default(), || {});
            return;
        }
        let wall = surface((
            "wall",
            WALL,
            Mat4::translation(Vec3::new(-0.5, -0.5, 0.0))
                .then(&Mat4::scaling(Vec3::new(WALL.width, WALL.height, 1.0)))
                .then(&Mat4::translation(Vec3::new(0.0, 0.0, NESTED.1))),
        ));
        let surface = Rc::clone(&surface);
        Box(wall, BoxSpec::default(), move || rooms(&surface, depth - 1));
    });
}

#[test]
fn an_eye_sees_through_doorways_as_deep_as_they_show() {
    let pressed: Presses = Rc::default();
    let sink = Rc::clone(&pressed);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let surface = pressable(Rc::clone(&sink));
        Box(
            Modifier::empty().fill_max_size().scene3d(|| Scene3d {
                camera: Camera::look_at(
                    Vec3::new(0.0, 0.0, -1000.0),
                    Vec3::ZERO,
                    Vec3::new(0.0, 1.0, 0.0),
                    FOCAL_LENGTH,
                ),
                light: None,
                eye: None,
            }),
            BoxSpec::default(),
            move || {
                let wall = surface((
                    "wall",
                    WALL,
                    Mat4::translation(Vec3::new(-0.5, -0.5, 0.0))
                        .then(&Mat4::scaling(Vec3::new(WALL.width, WALL.height, 1.0)))
                        .then(&Mat4::translation(FAR)),
                ));
                let surface = Rc::clone(&surface);
                // Five doorways, one inside another, past the eye's own
                // space: no depth below what shows on screen stops it.
                Box(wall, BoxSpec::default(), move || rooms(&surface, 4));
            },
        );
    });
    let reached = with_scene(&mut composition, |view, _, _| press_middle(&view, &pressed));
    assert_eq!(
        reached,
        Some("deep"),
        "a press through five doorways reaches the panel behind the last"
    );
}

#[test]
fn a_doorway_off_screen_shows_nothing_of_its_space() {
    let looking = Rc::new(Cell::new(0.0f32));
    let (turn, panel) = (Rc::clone(&looking), Rc::new(Cell::new(None)));
    let (scene, kept) = (Rc::new(Cell::new(None)), Rc::clone(&panel));
    let scene_node = Rc::clone(&scene);
    let pressed: Presses = Rc::default();
    let sink = Rc::clone(&pressed);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let surface = pressable(Rc::clone(&sink));
        let turn = Rc::clone(&turn);
        let kept = Rc::clone(&kept);
        let id = Box(
            Modifier::empty().fill_max_size().scene3d(move || Scene3d {
                camera: camera_looking(
                    Vec3::new(0.0, 0.0, -1000.0),
                    Vec3::new(turn.get().sin(), 0.0, turn.get().cos()),
                ),
                light: None,
                eye: None,
            }),
            BoxSpec::default(),
            move || {
                let wall = surface((
                    "wall",
                    WALL,
                    Mat4::translation(Vec3::new(-0.5, -0.5, 0.0))
                        .then(&Mat4::scaling(Vec3::new(WALL.width, WALL.height, 1.0)))
                        .then(&Mat4::translation(FAR)),
                ));
                let (surface, kept) = (Rc::clone(&surface), Rc::clone(&kept));
                Box(wall, BoxSpec::default(), move || {
                    kept.set(Some(window_into_nested(&surface).1));
                });
            },
        );
        scene_node.set(Some(id));
    });
    let (Some(scene), Some(panel)) = (scene.get(), panel.get()) else {
        panic!("the scene and the panel behind its doorway composed");
    };
    let (facing, turned_away) = with_scene(&mut composition, |mut view, applier, root| {
        let facing = view.find_target(panel).is_some();
        looking.set(std::f32::consts::FRAC_PI_2);
        camera_moved(&mut view, applier, (root, scene));
        (facing, view.find_target(panel).is_some())
    });
    assert!(
        facing,
        "the panel behind the doorway the eye faces takes input"
    );
    assert!(
        !turned_away,
        "with the doorway off screen, nothing behind it takes input"
    );
}

#[test]
fn a_ray_from_beside_the_eye_presses_the_panel_behind_a_doorway_where_it_meets_it() {
    let WalkRoom {
        mut composition,
        pressed,
        scene,
        ..
    } = walk_room(Vec3::new(0.0, 0.0, -1000.0));
    // The panel behind the doorway stands NESTED.0 + NESTED.1 behind the
    // wall's middle; the ray comes from the right of the eye, through the
    // doorway, to the panel's middle.
    let middle = Vec3::new(0.0, 0.0, FAR.z + NESTED.0 + NESTED.1);
    let origin = Vec3::new(150.0, 0.0, -1000.0);
    let ray = Ray {
        origin,
        direction: middle - origin,
    };
    with_scene(&mut composition, |view, _, _| {
        press_along(&view, scene, ray);
    });
    let pressed = pressed.borrow();
    let Some(&(name, local)) = pressed.first() else {
        panic!("the ray reached nothing");
    };
    assert_eq!(name, "nested");
    let center = Point::new(INNER.width * 0.5, INNER.height * 0.5);
    assert!(
        (local.x - center.x).abs() < 0.5 && (local.y - center.y).abs() < 0.5,
        "the press lands where the ray meets the panel, {local:?}, not {center:?}"
    );
}

#[test]
fn a_ray_through_a_wall_beside_its_doorway_reaches_the_wall() {
    let WalkRoom {
        mut composition,
        pressed,
        scene,
        ..
    } = walk_room(Vec3::new(0.0, 0.0, -1000.0));
    let ray = Ray {
        origin: Vec3::new(250.0, 150.0, -1000.0),
        direction: Vec3::new(0.0, 0.0, 1.0),
    };
    with_scene(&mut composition, |view, _, _| {
        press_along(&view, scene, ray);
    });
    assert_eq!(
        pressed
            .borrow()
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        ["wall"],
        "the wall, and only the wall, takes the ray beside its doorway"
    );
}

/// Presses and releases along `ray` in the scene of the node `scene`.
fn press_along(view: &cranpose_render_common::graph_scene::Scene, scene: NodeId, ray: Ray) {
    if let Some(target) = view.hit_test_ray(scene, ray).into_iter().next() {
        for kind in [PointerEventKind::Down, PointerEventKind::Up] {
            target.dispatch(
                PointerEvent::new(kind, Point::default(), Point::default()).with_ray(ray),
            );
        }
    }
}
