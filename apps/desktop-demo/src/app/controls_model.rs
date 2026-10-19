//! The controls of the controls tab as objects in a 3D room. Each part of a
//! control is a mesh built from the distance field the tab's stage shader
//! traces, shaded by the stage's own lights through a shader of its own:
//! it casts its shadow, it is solid, and the moving parts follow the
//! control's state. A part's texture lies on it as seen from straight
//! above its card, so where a pointer meets a part shows where it points
//! on the card, and every part takes the stage's input.

use std::{
    rc::Rc,
    sync::{Arc, OnceLock},
};

use cranpose_core::key;
use cranpose_ui::{
    composable,
    space::{Mat4, Material, Mesh, MeshSurface, Shaded, Vec3},
    Box, BoxSpec, Brush, Color, CornerRadii, DrawScope, Modifier, Point, Rect, Size,
};

use super::{
    animateControl, card_background, card_color, stage_input, stage_pointer, ControlHandle,
    ControlKind, StageAnimation, StageTouch, StageView, CAMERA_PITCH, CAMERA_YAW, SHADING_WGSL,
    SLIDER_LIFT, SLIDER_TRAVEL, STAGE_HEIGHT, WALL_STAGE_WIDTH,
};

/// The width of a control's card on a wall at scale one; with
/// [`STAGE_HEIGHT`] it sets the card's shape, which the parts' textures
/// take.
pub(crate) const WALL_CARD_WIDTH: f32 = 300.0;
/// How far in front of its card a control stands, so the card does not
/// show through its ground.
const LIFT: f32 = 1.0;
/// How many grid cells a part's distance field is sampled at along each
/// side of its box.
const RESOLUTION: u32 = 40;
// The tints the stage gives its two materials.
const METAL: Color = Color(0.94, 0.95, 0.97, 1.0);
const STEEL: Color = Color(0.62, 0.64, 0.68, 1.0);
const INK: Color = Color(0.10, 0.11, 0.13, 1.0);

/// One part of a control: its mesh, in its own coordinates, its colour and
/// how it moves.
struct Part {
    mesh: Arc<Mesh>,
    color: Color,
    motion: Motion,
}

/// How a part moves with its control's value and press, in the stage's
/// coordinates: x across the card, y out of it and z down it.
#[derive(Clone, Copy)]
enum Motion {
    /// Stands where its own coordinates put it.
    Still,
    /// Slides along x from `from` to `to` with the value, at height `lift`,
    /// a press pushing it `press` down.
    Slides {
        from: f32,
        to: f32,
        lift: f32,
        press: f32,
    },
    /// Turns about the vertical from `-sweep` to `sweep` with the value.
    Turns { sweep: f32, lift: f32, press: f32 },
    /// Tilts about the depth axis from `-sweep` to `sweep` with the value,
    /// about a point `pivot` up.
    Tilts { sweep: f32, pivot: f32, press: f32 },
    /// Grows from nothing about `at` with the value.
    Grows { at: Vec3, lift: f32, press: f32 },
}

impl Motion {
    /// Where the part stands for `value` and `press`, in the stage's
    /// coordinates.
    fn place(self, value: f32, press: f32) -> Mat4 {
        let lifted =
            |lift: f32, depth: f32| Mat4::translation(Vec3::new(0.0, lift - depth * press, 0.0));
        match self {
            Self::Still => Mat4::IDENTITY,
            Self::Slides {
                from,
                to,
                lift,
                press: depth,
            } => Mat4::translation(Vec3::new(from + (to - from) * value, 0.0, 0.0))
                .then(&lifted(lift, depth)),
            Self::Turns {
                sweep,
                lift,
                press: depth,
            } => Mat4::rotation_y(sweep * (2.0 * value - 1.0)).then(&lifted(lift, depth)),
            Self::Tilts {
                sweep,
                pivot,
                press: depth,
            } => Mat4::rotation_z(-sweep * (2.0 * value - 1.0)).then(&lifted(pivot, depth)),
            Self::Grows {
                at,
                lift,
                press: depth,
            } => {
                let size = value.clamp(0.0, 1.0);
                Mat4::translation(-at)
                    .then(&Mat4::scaling(Vec3::new(size, size, size)))
                    .then(&Mat4::translation(at))
                    .then(&lifted(lift, depth))
            }
        }
    }
}

/// A control's stage, in its own units, placed on a card of `size`: x
/// across the card, y out of the wall toward the room, z down the card.
fn stage_to_card(size: Size) -> Mat4 {
    let unit = size.width / WALL_STAGE_WIDTH;
    Mat4::from_axes(
        Vec3::new(unit, 0.0, 0.0),
        Vec3::new(0.0, 0.0, -unit),
        Vec3::new(0.0, unit, 0.0),
        Vec3::new(size.width * 0.5, size.height * 0.5, -LIFT),
    )
}

/// Where the stage point `(x, z)` lies on its card, as a texture
/// coordinate.
fn card_uv(x: f32, z: f32) -> [f32; 2] {
    let aspect = WALL_CARD_WIDTH / STAGE_HEIGHT;
    [
        x / WALL_STAGE_WIDTH + 0.5,
        z * aspect / WALL_STAGE_WIDTH + 0.5,
    ]
}

/// The part where `field` is zero within `bounds`, its texture as seen
/// from above its card.
fn part(
    field: impl Fn(Vec3) -> f32,
    bounds: (Vec3, Vec3),
    (color, motion): (Color, Motion),
) -> Option<Part> {
    let shape = Mesh::from_sdf(field, bounds, RESOLUTION).ok()?;
    let uvs = shape
        .positions()
        .iter()
        .map(|&[x, _, z]| card_uv(x, z))
        .collect();
    let mesh = Mesh::new(
        shape.positions().to_vec(),
        uvs,
        Some(shape.normals().to_vec()),
        shape.indices().to_vec(),
    )
    .ok()?;
    Some(Part {
        mesh: Arc::new(mesh),
        color,
        motion,
    })
}

fn round_box(p: Vec3, b: Vec3, r: f32) -> f32 {
    let q = Vec3::new(
        p.x.abs() - b.x + r,
        p.y.abs() - b.y + r,
        p.z.abs() - b.z + r,
    );
    q.max(Vec3::ZERO).length() + q.x.max(q.y.max(q.z)).min(0.0) - r
}

fn capsule(p: Vec3, a: Vec3, b: Vec3, r: f32) -> f32 {
    let (offset, axis) = (p - a, b - a);
    let along = (offset.dot(axis) / axis.dot(axis).max(1e-6)).clamp(0.0, 1.0);
    (offset - axis * along).length() - r
}

fn round_cylinder(p: Vec3, radius: f32, half_height: f32, edge: f32) -> f32 {
    let d = (
        p.x.hypot(p.z) - (radius - edge),
        p.y.abs() - (half_height - edge),
    );
    d.0.max(d.1).min(0.0) + d.0.max(0.0).hypot(d.1.max(0.0)) - edge
}

fn sphere(p: Vec3, r: f32) -> f32 {
    p.length() - r
}

/// `d` cut by a large sphere whose top lies at `top`: a gently domed top.
fn crowned(d: f32, p: Vec3, top: f32, curve: f32) -> f32 {
    d.max(sphere(p - Vec3::new(0.0, top - curve, 0.0), curve))
}

fn segment_2d((x, z): (f32, f32), (ax, az): (f32, f32), (bx, bz): (f32, f32)) -> f32 {
    let (offset, axis) = ((x - ax, z - az), (bx - ax, bz - az));
    let along = ((offset.0 * axis.0 + offset.1 * axis.1)
        / (axis.0 * axis.0 + axis.1 * axis.1).max(1e-6))
    .clamp(0.0, 1.0);
    (offset.0 - axis.0 * along).hypot(offset.1 - axis.1 * along)
}

/// The box from `-half` to `half`.
fn around(half: Vec3) -> (Vec3, Vec3) {
    (-half, half)
}

/// The parts of `kind`, built the first time a room shows it.
fn parts(kind: ControlKind) -> &'static [Part] {
    static PARTS: [OnceLock<Vec<Part>>; 6] = [const { OnceLock::new() }; 6];
    PARTS[kind as usize].get_or_init(|| build(kind).into_iter().flatten().collect())
}

/// The parts of `kind`, from the distance fields of the stage shader.
fn build(kind: ControlKind) -> Vec<Option<Part>> {
    match kind {
        ControlKind::Checkmark => {
            let (a, b, c) = (
                Vec3::new(-0.48, 0.0, -0.10),
                Vec3::new(-0.14, 0.0, 0.50),
                Vec3::new(0.52, 0.0, -0.62),
            );
            vec![part(
                move |p| capsule(p, a, b, 0.085).min(capsule(p, b, c, 0.085)),
                (Vec3::new(-0.6, -0.1, -0.75), Vec3::new(0.65, 0.1, 0.62)),
                (
                    METAL,
                    Motion::Grows {
                        at: b,
                        lift: 0.105,
                        press: 0.020,
                    },
                ),
            )]
        }
        ControlKind::Slider => {
            let end = SLIDER_TRAVEL + 0.22;
            vec![
                part(
                    move |p| {
                        capsule(
                            p,
                            Vec3::new(-end, 0.052, 0.0),
                            Vec3::new(end, 0.052, 0.0),
                            0.025,
                        )
                    },
                    (
                        Vec3::new(-end - 0.05, 0.0, -0.05),
                        Vec3::new(end + 0.05, 0.1, 0.05),
                    ),
                    (STEEL, Motion::Still),
                ),
                part(
                    |p| {
                        let puck = crowned(round_cylinder(p, 0.285, 0.105, 0.058), p, 0.105, 2.4);
                        let groove = round_box(
                            p - Vec3::new(0.0, 0.100, 0.0),
                            Vec3::new(0.130, 0.018, 0.012),
                            0.010,
                        );
                        puck.max(-groove)
                    },
                    around(Vec3::new(0.31, 0.13, 0.31)),
                    (
                        METAL,
                        Motion::Slides {
                            from: -SLIDER_TRAVEL,
                            to: SLIDER_TRAVEL,
                            lift: SLIDER_LIFT,
                            press: 0.014,
                        },
                    ),
                ),
            ]
        }
        ControlKind::Toggle => vec![
            part(
                |p| {
                    let outline = (segment_2d((p.x, p.z), (-0.26, 0.0), (0.26, 0.0)) - 0.30).abs();
                    outline.hypot(p.y - 0.040) - 0.026
                },
                (Vec3::new(-0.62, 0.0, -0.36), Vec3::new(0.62, 0.08, 0.36)),
                (STEEL, Motion::Still),
            ),
            part(
                |p| round_box(p, Vec3::new(0.30, 0.115, 0.255), 0.115),
                around(Vec3::new(0.33, 0.14, 0.29)),
                (
                    METAL,
                    Motion::Slides {
                        from: -0.26,
                        to: 0.26,
                        lift: 0.160,
                        press: 0.012,
                    },
                ),
            ),
        ],
        ControlKind::PushButton => vec![part(
            |p| round_box(p, Vec3::new(0.42, 0.135, 0.275), 0.135),
            around(Vec3::new(0.45, 0.16, 0.30)),
            (
                METAL,
                Motion::Slides {
                    from: 0.0,
                    to: 0.0,
                    lift: 0.185,
                    press: 0.070,
                },
            ),
        )],
        // The knob is round, so stands still where the pointer meets it;
        // only its mark turns with the value.
        ControlKind::VolumeDial => vec![
            part(
                |p| crowned(round_cylinder(p, 0.42, 0.168, 0.072), p, 0.168, 1.9),
                around(Vec3::new(0.45, 0.19, 0.45)),
                (
                    METAL,
                    Motion::Slides {
                        from: 0.0,
                        to: 0.0,
                        lift: 0.168,
                        press: 0.022,
                    },
                ),
            ),
            part(
                |p| {
                    round_box(
                        p - Vec3::new(0.0, 0.168, -0.285),
                        Vec3::new(0.032, 0.03, 0.12),
                        0.024,
                    )
                },
                (Vec3::new(-0.06, 0.12, -0.43), Vec3::new(0.06, 0.22, -0.14)),
                (
                    INK,
                    Motion::Turns {
                        sweep: 2.36,
                        lift: 0.168,
                        press: 0.022,
                    },
                ),
            ),
        ],
        ControlKind::LeverSwitch => vec![
            part(
                |p| round_cylinder(p - Vec3::new(0.0, 0.048, 0.0), 0.27, 0.048, 0.044),
                (Vec3::new(-0.3, -0.01, -0.3), Vec3::new(0.3, 0.11, 0.3)),
                (STEEL, Motion::Still),
            ),
            part(
                |p| sphere(p - Vec3::new(0.0, -0.02, 0.0), 0.19),
                (Vec3::new(-0.21, -0.23, -0.21), Vec3::new(0.21, 0.19, 0.21)),
                (METAL, Motion::Still),
            ),
            part(
                |p| {
                    let stick = capsule(p, Vec3::ZERO, Vec3::new(0.0, 0.40, 0.0), 0.05);
                    stick.min(sphere(p - Vec3::new(0.0, 0.45, 0.0), 0.105))
                },
                (Vec3::new(-0.13, -0.07, -0.13), Vec3::new(0.13, 0.58, 0.13)),
                (
                    METAL,
                    Motion::Tilts {
                        sweep: 0.52,
                        pivot: 0.09,
                        press: 0.018,
                    },
                ),
            ),
        ],
    }
}

/// Where a pointer on a part of a card of `size`, at a point of the part's
/// texture, points on the card, with the part placed by `place`: the
/// texture lies on the part where it stood in its own coordinates.
fn part_touch(size: Size, place: impl Fn() -> Mat4 + 'static) -> StageTouch {
    let unit = size.width / WALL_STAGE_WIDTH;
    StageTouch {
        view: StageView::Above,
        to_stage: Some(Rc::new(move |point: Point| {
            let own = Vec3::new(
                (point.x - size.width * 0.5) / unit,
                0.0,
                (point.y - size.height * 0.5) / unit,
            );
            let placed = place().transform_point(own);
            Point::new(
                placed.x * unit + size.width * 0.5,
                placed.z * unit + size.height * 0.5,
            )
        })),
        clicks: true,
    }
}

/// A control of `kind` on its card of `size`, laid out by `modifier` where
/// the card lies on its wall: the card, with the marks the stage draws on
/// its ground, and the control's parts standing out of it.
#[composable]
pub(crate) fn ControlModel(
    kind: ControlKind,
    control: ControlHandle,
    modifier: Modifier,
    size: Size,
) {
    let animation = animateControl(kind, control.clone());
    let card = card_background(modifier.clone().size(size), true);
    let marks = control.clone();
    Box(
        card.draw_behind(move |scope| {
            ground_marks(scope, kind, marks.state().get().scene_value(kind), size);
        })
        .then(stage_input(kind, &control, StageView::Above)),
        BoxSpec::default(),
        || {},
    );
    for index in 0..parts(kind).len() {
        let (control, modifier) = (control.clone(), modifier.clone());
        key(index, move || {
            PartModel((kind, index), (control, animation), (modifier, size));
        });
    }
}

/// Part `index` of a control of `kind`, standing where its control's state
/// places it.
#[composable(no_skip)]
fn PartModel(
    (kind, index): (ControlKind, usize),
    (control, animation): (ControlHandle, StageAnimation),
    (modifier, size): (Modifier, Size),
) {
    let Some(part) = parts(kind).get(index) else {
        return;
    };
    let place = move || {
        part.motion
            .place(animation.value.value(), animation.press.value())
    };
    let content = if kind == ControlKind::PushButton {
        modifier.draw_behind(move |scope| power_glyph(scope, size))
    } else {
        modifier
    };
    Box(
        content
            .required_size(size)
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&part.mesh),
                transform: place().then(&stage_to_card(size)),
                material: part_material(),
                casts_shadow: true,
                solid: true,
            })
            .background(part.color)
            .then(stage_pointer(kind, &control, part_touch(size, place))),
        BoxSpec::default(),
        || {},
    );
}

/// How a part shows in the room: as the dark stage shows it, mirroring the
/// stage's lights and its card.
fn part_material() -> Material {
    static MATERIAL: OnceLock<Material> = OnceLock::new();
    MATERIAL
        .get_or_init(|| {
            let card = card_color(true);
            let mut params = [0.0; Shaded::PARAMS];
            params[..4].copy_from_slice(&[1.0, card.0, card.1, card.2]);
            for (row, values) in tab_view_turn().iter().zip(params[4..13].chunks_mut(3)) {
                values.copy_from_slice(row);
            }
            Material::Shaded(Shaded {
                shader: Arc::from(format!(
                    "{SHADING_WGSL}{}",
                    include_str!("controls_part.wgsl")
                )),
                params,
            })
        })
        .clone()
}

/// The turn, as rows, that takes the room's view of a card, straight into
/// it with the wall's up up the screen, to the view the controls tab's
/// camera has of its stage: a part seen straight on in the room mirrors the
/// stage's lights as the tab shows them.
fn tab_view_turn() -> [[f32; 3]; 3] {
    let (yaw, pitch) = (CAMERA_YAW, CAMERA_PITCH);
    let forward = -Vec3::new(
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        yaw.cos() * pitch.cos(),
    );
    let vertical = Vec3::new(0.0, 1.0, 0.0);
    let up = (vertical - forward * vertical.dot(forward))
        .normalized()
        .unwrap_or(vertical);
    let tab = [forward.cross(up), up, forward];
    let room_forward = Vec3::new(0.0, -1.0, 0.0);
    let room_up = Vec3::new(0.0, 0.0, -1.0);
    let room = [room_forward.cross(room_up), room_up, room_forward];
    let axis = |v: Vec3| [v.x, v.y, v.z];
    std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            tab.iter()
                .zip(&room)
                .map(|(&to, &from)| axis(to)[i] * axis(from)[j])
                .sum()
        })
    })
}

/// The marks the stage draws on its ground for `kind` at `value`: the
/// plate under the control, the dial's ticks and the lever's two stops.
fn ground_marks(scope: &mut dyn DrawScope, kind: ControlKind, value: f32, size: Size) {
    let unit = size.width / WALL_STAGE_WIDTH;
    let at = |x: f32, z: f32| Point::new(x * unit + size.width * 0.5, z * unit + size.height * 0.5);
    let (half, radius) = match kind {
        ControlKind::Checkmark => ((0.74, 0.78), 0.22),
        ControlKind::Slider => ((1.02, 0.24), 0.24),
        ControlKind::Toggle => ((0.72, 0.40), 0.38),
        ControlKind::PushButton => ((0.78, 0.52), 0.26),
        ControlKind::VolumeDial => ((0.78, 0.78), 0.78),
        ControlKind::LeverSwitch => ((0.70, 0.44), 0.22),
    };
    let corner = at(-half.0, -half.1);
    let plate = Rect {
        x: corner.x,
        y: corner.y,
        width: 2.0 * half.0 * unit,
        height: 2.0 * half.1 * unit,
    };
    let rounding = CornerRadii::uniform(radius * unit);
    scope.draw_round_rect_at(plate, Brush::solid(Color(0.74, 0.78, 0.88, 0.07)), rounding);
    let idle = Color(0.36, 0.38, 0.44, 0.8);
    let live = Color(0.88, 0.90, 0.96, 0.92);
    match kind {
        ControlKind::VolumeDial => {
            for step in 0..=10 {
                let share = step as f32 / 10.0;
                let angle = 2.36 * (2.0 * share - 1.0);
                let color = if share <= value + 0.001 { live } else { idle };
                scope.draw_circle(
                    Brush::solid(color),
                    at(0.58 * angle.sin(), -0.58 * angle.cos()),
                    0.022 * unit,
                );
            }
        }
        ControlKind::LeverSwitch => {
            for (x, on) in [(-0.50, false), (0.50, true)] {
                let color = if (value >= 0.5) == on { live } else { idle };
                let corner = at(x - 0.055, -0.02 - 0.013);
                let stop = Rect {
                    x: corner.x,
                    y: corner.y,
                    width: 0.11 * unit,
                    height: 0.026 * unit,
                };
                scope.draw_round_rect_at(
                    stop,
                    Brush::solid(color),
                    CornerRadii::uniform(0.013 * unit),
                );
            }
        }
        _ => {}
    }
}

/// The power symbol on the push button's top, in the middle of its card.
fn power_glyph(scope: &mut dyn DrawScope, size: Size) {
    let unit = size.width / WALL_STAGE_WIDTH;
    let middle = Point::new(size.width * 0.5, size.height * 0.5);
    let ring = 0.082 * unit;
    scope.draw_circle(Brush::solid(INK), middle, ring + 0.013 * unit);
    scope.draw_circle(Brush::solid(METAL), middle, ring - 0.013 * unit);
    let bar = Rect {
        x: middle.x - 0.026 * unit,
        y: middle.y - 0.13 * unit,
        width: 0.052 * unit,
        height: 0.13 * unit,
    };
    scope.draw_rect_at(bar, Brush::solid(METAL));
    let stroke = Rect {
        x: middle.x - 0.012 * unit,
        y: middle.y - 0.11 * unit,
        width: 0.024 * unit,
        height: 0.11 * unit,
    };
    scope.draw_round_rect_at(
        stroke,
        Brush::solid(INK),
        CornerRadii::uniform(0.012 * unit),
    );
}
