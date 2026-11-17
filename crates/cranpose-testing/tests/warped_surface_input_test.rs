// Content a mesh surface shows bent, as the floor of a pool shows through
// moving water, takes a click at the content point that shows where the
// pointer is: the mesh's texture coordinates carry the bend, so the pointer
// follows it with no map of its own.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    Box, BoxSpec, Modifier, Point, Size,
    space::{Mat4, Material, Mesh, MeshSurface, Vec3},
};

const PANEL: Size = Size {
    width: 200.0,
    height: 120.0,
};
// Where the panel's top left corner stands; the camera shows the plane
// z = 0 at its own size, so a scene point is a window point.
const PANEL_AT: Vec3 = Vec3::new(60.0, 40.0, 0.0);
// Grid cells across and down.
const CELLS: (usize, usize) = (20, 12);

// The content point that shows at `shown`, a point of the panel: shifted
// by a wave, far more in some places than in others, as refraction does.
fn bent(shown: Point) -> Point {
    let (u, v) = (shown.x / PANEL.width, shown.y / PANEL.height);
    Point::new(
        shown.x + 9.0 * (u * 7.0 + v * 3.0).sin() * v,
        shown.y + 6.0 * (u * 5.0 - v * 4.0).cos() * u,
    )
}

// A flat grid whose vertices stand where they show and whose texture
// coordinates are the content points shown there.
fn bent_grid() -> Mesh {
    let (columns, rows) = CELLS;
    let mut positions = Vec::with_capacity((columns + 1) * (rows + 1));
    let mut uvs = Vec::with_capacity(positions.capacity());
    for row in 0..=rows {
        for column in 0..=columns {
            let (u, v) = (column as f32 / columns as f32, row as f32 / rows as f32);
            let content = bent(Point::new(u * PANEL.width, v * PANEL.height));
            positions.push([u, v, 0.0]);
            uvs.push([content.x / PANEL.width, content.y / PANEL.height]);
        }
    }
    let mut indices = Vec::with_capacity(columns * rows * 6);
    for row in 0..rows {
        for column in 0..columns {
            let top_left = (row * (columns + 1) + column) as u32;
            let bottom_left = top_left + columns as u32 + 1;
            indices.extend([
                top_left,
                bottom_left,
                top_left + 1,
                top_left + 1,
                bottom_left,
                bottom_left + 1,
            ]);
        }
    }
    Mesh::new(positions, uvs, None, indices).expect("a whole grid")
}

#[test]
fn a_click_on_bent_content_lands_on_the_point_shown_there() {
    let clicks: Rc<RefCell<Vec<Point>>> = Rc::default();
    let sink = Rc::clone(&clicks);
    let mesh = Arc::new(bent_grid());
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let (sink, mesh) = (Rc::clone(&sink), Arc::clone(&mesh));
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .scene3d(|| crate::scene_facing::facing(Vec3::new(160.0, 100.0, 0.0))),
                BoxSpec::default(),
                move || {
                    let (sink, mesh) = (Rc::clone(&sink), Arc::clone(&mesh));
                    Box(
                        Modifier::empty()
                            .required_size(PANEL)
                            .mesh_surface(move || MeshSurface {
                                mesh: Arc::clone(&mesh),
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
        },
    );
    crate::test_window::first_frame(&mut shell, Size::new(320.0, 200.0));

    // A vertex, and points inside cells, where the bend is strong and where
    // it is weak.
    let shown = [
        Point::new(100.0, 60.0),
        Point::new(37.3, 101.9),
        Point::new(181.4, 13.7),
        Point::new(150.6, 110.2),
    ];
    for (press, at) in shown.iter().enumerate() {
        let millis = 100 + 100 * press as i64;
        let (x, y) = (PANEL_AT.x + at.x, PANEL_AT.y + at.y);
        shell.set_cursor_at_time(x, y, Some(millis));
        shell.pointer_pressed_at_time(Some(millis));
        shell.pointer_released_at_position_time(x, y, Some(millis + 30));
        shell.update_at_frame_time_nanos(32_000_000 + 16_000_000 * press as u64);
    }

    let clicks = clicks.borrow();
    assert_eq!(clicks.len(), shown.len(), "every click reached the panel");
    // Inside a cell the grid carries the bend on linearly: within a fraction
    // of a pixel of the wave at this grid size.
    for (at, click) in shown.iter().zip(clicks.iter()) {
        let expected = bent(*at);
        assert!(
            (expected.x - at.x).abs().max((expected.y - at.y).abs()) > 1.5,
            "the wave bends {at:?} too little to tell"
        );
        assert!(
            (click.x - expected.x).abs() < 0.6 && (click.y - expected.y).abs() < 0.6,
            "a click where {at:?} shows lands on {click:?}, not on the bent point {expected:?}"
        );
    }
}
