//! Inside a scene, a layer's 3D frame comes from the composition: its place
//! in its parent's layout, and the 3D transforms of the layers above it.

use std::rc::Rc;

use cranpose_ui::{
    Modifier, Point, Size,
    space::{Camera, Mat4, Scene3d, Vec3},
    widgets::{Box, BoxSpec, Row, RowSpec},
};

use crate::mesh_surface_hits::{
    FOCAL_LENGTH, Pressable, Presses, VIEWPORT, press_at, pressable, with_scene,
};

const CARD: Size = Size {
    width: 200.0,
    height: 150.0,
};

/// A camera one focal length in front of the plane z = 0, looking at the
/// viewport's middle: it shows the plane at its own size, where layout put
/// what lies on it.
fn flat_camera() -> Camera {
    let middle = Vec3::new(VIEWPORT.width * 0.5, VIEWPORT.height * 0.5, 0.0);
    Camera::look_at(
        middle - Vec3::new(0.0, 0.0, FOCAL_LENGTH),
        middle,
        Vec3::new(0.0, 1.0, 0.0),
        FOCAL_LENGTH,
    )
}

/// A card that fills its node, at the node's place.
fn card(surface: &Pressable, name: &'static str) -> Modifier {
    surface((
        name,
        CARD,
        Mat4::scaling(Vec3::new(CARD.width, CARD.height, 1.0)),
    ))
}

/// The names of the surfaces pressed at each of `points` in a scene seen
/// through [`flat_camera`], whose content `inside` composes.
fn pressed_at(
    inside: impl Fn(&Pressable) + 'static,
    points: &[Point],
) -> Vec<Option<&'static str>> {
    let pressed: Presses = Rc::default();
    let sink = Rc::clone(&pressed);
    let inside = Rc::new(inside);
    let mut composition = cranpose_ui::run_test_composition(move || {
        let surface = pressable(Rc::clone(&sink));
        let inside = Rc::clone(&inside);
        Box(
            Modifier::empty().fill_max_size().scene3d(|| Scene3d {
                camera: flat_camera(),
                light: None,
                eye: None,
            }),
            BoxSpec::default(),
            move || inside(&surface),
        );
    });
    with_scene(&mut composition, |scene, _, _| {
        points
            .iter()
            .map(|&at| {
                pressed.borrow_mut().clear();
                press_at(&scene, at);
                pressed.borrow().first().map(|(name, _)| *name)
            })
            .collect()
    })
}

#[test]
fn surfaces_laid_out_in_a_row_stand_where_layout_puts_them() {
    let reached = pressed_at(
        |surface| {
            let surface = Rc::clone(surface);
            Row(Modifier::empty(), RowSpec::default(), move || {
                Box(card(&surface, "first"), BoxSpec::default(), || {});
                Box(card(&surface, "second"), BoxSpec::default(), || {});
            });
        },
        &[
            Point::new(CARD.width * 0.5, CARD.height * 0.5),
            Point::new(CARD.width * 1.5, CARD.height * 0.5),
            Point::new(CARD.width * 2.5, CARD.height * 0.5),
        ],
    );
    assert_eq!(
        reached,
        [Some("first"), Some("second"), None],
        "each card stands in 3D where the row laid it out"
    );
}

#[test]
fn a_3d_transform_moves_the_surfaces_beneath_it() {
    let reached = pressed_at(
        |surface| {
            let surface = Rc::clone(surface);
            Box(
                Modifier::empty().transform3d(|| Mat4::translation(Vec3::new(300.0, 100.0, 0.0))),
                BoxSpec::default(),
                move || {
                    Box(card(&surface, "moved"), BoxSpec::default(), || {});
                },
            );
        },
        &[
            Point::new(300.0 + CARD.width * 0.5, 100.0 + CARD.height * 0.5),
            Point::new(CARD.width * 0.5, CARD.height * 0.5),
        ],
    );
    assert_eq!(
        reached,
        [Some("moved"), None],
        "the card moves with the transform above it, from where layout put it"
    );
}
