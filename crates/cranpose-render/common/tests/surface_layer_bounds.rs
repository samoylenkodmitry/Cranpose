// A mesh surface covers what the modifiers after it lay out, as a Compose
// graphics layer covers the coordinator after it: a node its parent holds
// narrower than its content still gives the mesh all of its content, and a
// padding before the surface insets the content the mesh shows.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_ui::{
    Alignment, Modifier, Point, Size,
    space::{Material, Mesh, MeshSurface, Scene3d, Vec3},
    widgets::{Box, BoxSpec},
};

use crate::mesh_surface_hits::{FAR, VIEWPORT, WALL, camera, centered, press_at, with_scene};

// The point a press reaches on the wall's content, which a pressable box
// fills, where the eye sees the point `uv` of the wall's mesh. The wall's
// node takes `node` before its surface, in a parent of `parent`'s size.
fn pressed_on_wall(parent: Size, node: Modifier, uv: (f32, f32)) -> Option<Point> {
    let pressed: Rc<RefCell<Vec<Point>>> = Rc::default();
    let sink = Rc::clone(&pressed);
    let plane = Arc::new(Mesh::plane());
    let wall = node.mesh_surface(move || MeshSurface {
        mesh: Arc::clone(&plane),
        transform: centered(WALL, FAR),
        material: Material::Unlit,
        casts_shadow: false,
        solid: true,
    });
    let content = Modifier::empty()
        .fill_max_size()
        .clickable(move |point| sink.borrow_mut().push(point));
    let mut composition = cranpose_ui::run_test_composition(move || {
        let (wall, content) = (wall.clone(), content.clone());
        Box(
            Modifier::empty().fill_max_size().scene3d(|| Scene3d {
                camera: camera(),
                light: None,
                eye: None,
            }),
            BoxSpec::default(),
            move || {
                let (wall, content) = (wall.clone(), content.clone());
                Box(
                    Modifier::empty().size(parent),
                    BoxSpec::default(),
                    move || {
                        let content = content.clone();
                        Box(wall.clone(), BoxSpec::default(), move || {
                            Box(content.clone(), BoxSpec::default(), || {});
                        });
                    },
                );
            },
        );
    });
    let shown = camera()
        .project(
            centered(WALL, FAR).transform_point(Vec3::new(uv.0, uv.1, 0.0)),
            VIEWPORT,
        )
        .expect("the wall is in front of the eye");
    with_scene(&mut composition, |scene, _, _| press_at(&scene, shown));
    pressed.borrow().first().copied()
}

fn assert_near(pressed: Option<Point>, expected: Point, what: &str) {
    let Some(pressed) = pressed else {
        panic!("{what}: the press reached no content");
    };
    assert!(
        (pressed.x - expected.x).abs() < 1.0 && (pressed.y - expected.y).abs() < 1.0,
        "{what}: the press reached {pressed:?}, not {expected:?}"
    );
}

#[test]
fn a_surface_its_parent_holds_narrower_spans_all_of_its_content() {
    let pressed = pressed_on_wall(
        Size::new(100.0, 100.0),
        Modifier::empty()
            .wrap_content_size(Alignment::TOP_START, true)
            .size(WALL),
        (0.75, 0.75),
    );
    assert_near(
        pressed,
        Point::new(WALL.width * 0.75, WALL.height * 0.75),
        "the mesh spans the wall's whole content, past its parent's width",
    );
}

#[test]
fn a_surface_after_a_padding_spans_the_padded_content() {
    const PADDING: f32 = 50.0;
    let pressed = pressed_on_wall(
        VIEWPORT,
        Modifier::empty()
            .required_size(Size::new(
                WALL.width + 2.0 * PADDING,
                WALL.height + 2.0 * PADDING,
            ))
            .padding(PADDING),
        (0.25, 0.25),
    );
    assert_near(
        pressed,
        Point::new(WALL.width * 0.25, WALL.height * 0.25),
        "the mesh spans the content inside the padding",
    );
}
