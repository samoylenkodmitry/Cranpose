//! A shape that shifts from a ball to a rounded box to a ring and back,
//! drawn from a signed distance field and built again as it changes. Its
//! content stays live: its button takes clicks where it shows on the shape.

use std::{cell::Cell, rc::Rc, sync::Arc};

use cranpose_core::rememberMutableStateOf;
use cranpose_ui::{
    composable,
    space::{Mat4, Material, Mesh, MeshSurface, Vec3},
    Alignment, Box, BoxSpec, Brush, Button, ButtonSpec, Color, Column, ColumnSpec, Modifier, Point,
    Size, Text,
};

use super::{
    space::{RoomShape, RoomState},
    text_style, Ticking,
};

const BLOB_CONTENT: Size = Size {
    width: 512.0,
    height: 256.0,
};
/// The cells along each side of the box the field is sampled in.
const RESOLUTION: u32 = 36;
/// How often the shape is built again, in seconds.
const REBUILD_EVERY: f32 = 1.0 / 30.0;
/// The shapes it passes through in a second.
const SHAPES_PER_SECOND: f32 = 0.3;
const FIELD_REACH: f32 = 1.1;

#[composable(no_skip)]
pub(super) fn Blob(room: RoomState, shape: RoomShape) {
    let time = rememberMutableStateOf(|| 0.0f32);
    let mesh = rememberMutableStateOf(|| blob_mesh(0.0));
    let presses = rememberMutableStateOf(|| 0u32);
    let since_built = Rc::new(Cell::new(0.0f32));
    Ticking(room.motion.get(), move |seconds| {
        time.update(|now| *now += seconds);
        since_built.set(since_built.get() + seconds);
        if since_built.get() >= REBUILD_EVERY {
            since_built.set(0.0);
            if let Some(next) = blob_mesh(time.get()) {
                mesh.set(Some(next));
            }
        }
    });
    let radius = shape.width.min(shape.height) * 0.11;
    let center = Vec3::new(shape.width * 0.84, shape.height * 0.6, shape.depth * 0.5);
    let Some(current) = mesh.get() else {
        return;
    };
    Box(
        Modifier::empty()
            .required_size(BLOB_CONTENT)
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&current),
                transform: Mat4::scaling(Vec3::new(radius, radius, radius))
                    .then(&Mat4::rotation_y(time.get() * 0.4))
                    .then(&Mat4::translation(center)),
                material: Material::Lit,
                casts_shadow: true,
                solid: true,
            })
            .draw_behind(|scope| {
                let size = scope.size();
                scope.draw_rect(Brush::linear_gradient_range(
                    vec![
                        Color(0.95, 0.35, 0.45, 1.0),
                        Color(0.35, 0.3, 0.95, 1.0),
                        Color(0.2, 0.85, 0.75, 1.0),
                    ],
                    Point::new(0.0, 0.0),
                    Point::new(size.width, size.height),
                ));
            }),
        BoxSpec {
            content_alignment: Alignment::CENTER,
            ..BoxSpec::default()
        },
        move || {
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                Text(
                    "a shape from a distance field",
                    Modifier::empty().padding(4.0),
                    text_style(18.0, Color::WHITE),
                );
                Button(
                    Modifier::empty()
                        .rounded_corners(10.0)
                        .background(Color(0.1, 0.1, 0.16, 0.85))
                        .padding(10.0),
                    ButtonSpec::default(),
                    move || presses.set(presses.get() + 1),
                    move || {
                        Text(
                            format!("Clicked {} times", presses.get()),
                            Modifier::empty(),
                            text_style(16.0, Color::WHITE),
                        );
                    },
                );
            });
        },
    );
}

/// The shape `time` seconds in, or `None` should its field leave no
/// surface in the box.
fn blob_mesh(time: f32) -> Option<Arc<Mesh>> {
    let reach = Vec3::new(FIELD_REACH, FIELD_REACH, FIELD_REACH);
    Mesh::from_sdf(
        move |point| blob_field(point, time),
        (-reach, reach),
        RESOLUTION,
    )
    .ok()
    .map(Arc::new)
}

/// The distance to the shape at `time`: a blend of the two shapes it passes
/// between, with a ripple that runs over it.
fn blob_field(point: Vec3, time: f32) -> f32 {
    let phase = (time * SHAPES_PER_SECOND).rem_euclid(3.0);
    let from = phase.floor() as usize;
    let blend = {
        let t = phase.fract();
        t * t * (3.0 - 2.0 * t)
    };
    let shapes = [ball(point), rounded_box(point), ring(point)];
    let distance = shapes[from] + (shapes[(from + 1) % 3] - shapes[from]) * blend;
    let ripple = (point.x * 6.0 + time * 2.0).sin()
        * (point.y * 6.0 + time * 1.7).sin()
        * (point.z * 6.0 + time * 1.3).sin();
    distance + 0.05 * ripple
}

fn ball(point: Vec3) -> f32 {
    point.length() - 0.75
}

fn rounded_box(point: Vec3) -> f32 {
    let (half, round) = (0.5, 0.18);
    let q = Vec3::new(
        point.x.abs() - half,
        point.y.abs() - half,
        point.z.abs() - half,
    );
    let outside = Vec3::new(q.x.max(0.0), q.y.max(0.0), q.z.max(0.0)).length();
    outside + q.x.max(q.y).max(q.z).min(0.0) - round
}

/// A ring facing the viewer.
fn ring(point: Vec3) -> f32 {
    let around = (point.x * point.x + point.y * point.y).sqrt() - 0.58;
    (around * around + point.z * point.z).sqrt() - 0.26
}
