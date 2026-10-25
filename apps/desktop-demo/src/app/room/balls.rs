//! The room's two balls that are not screens: one of glass, which bends the
//! room behind it, and one of fire, which flies around the room in a glow
//! of its own.

use std::sync::{Arc, OnceLock};

use cranpose_core::{rememberMutableStateOf, MutableState};
use cranpose_ui::{
    composable,
    space::{Camera, Glass, Mat4, Material, Mesh, MeshSurface, Vec3},
    text::TextUnit,
    Alignment, Box, BoxSpec, Brush, Color, GraphicsLayer, Modifier, Point, Size, Text, TextStyle,
};
use cranpose_ui_graphics::{RenderEffect, RuntimeShader, TileMode, RUNTIME_SHADER_PRELUDE_WGSL};

use super::{
    space::{RoomShape, RoomState},
    Ticking,
};
use crate::app::shader_rect::WGSL_HELPERS;

const GLASS_CONTENT: Size = Size {
    width: 512.0,
    height: 256.0,
};
const FIRE_CONTENT: Size = Size {
    width: 512.0,
    height: 256.0,
};
const HALO_CONTENT: Size = Size {
    width: 256.0,
    height: 256.0,
};
/// How far around its path the fire flies in a second, in radians.
const ORBIT_PER_SECOND: f32 = 0.4;
/// The halo's side, against the fire's radius.
const HALO_PER_RADIUS: f32 = 3.6;

/// A ball of clear glass in front of the far wall: the text behind it shows
/// through it upside down, and its label stays live on its glass. Glass
/// lets most of the light through, so it casts no shadow.
#[composable(no_skip)]
pub(super) fn GlassBall(shape: RoomShape, mesh: Arc<Mesh>) {
    let radius = shape.width.min(shape.height) * 0.12;
    let center = Vec3::new(
        shape.width * 0.62,
        shape.height * 0.3,
        shape.depth - radius * 1.6,
    );
    Box(
        Modifier::empty()
            .required_size(GLASS_CONTENT)
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&mesh),
                transform: Mat4::scaling(Vec3::new(radius, radius, radius))
                    .then(&Mat4::translation(center)),
                material: Material::Glass(Glass::clear(2.0 * radius)),
                casts_shadow: false,
                solid: true,
            }),
        BoxSpec {
            content_alignment: Alignment::CENTER,
            ..BoxSpec::default()
        },
        || {
            Text(
                "glass",
                Modifier::empty(),
                TextStyle {
                    span_style: cranpose_ui::text::SpanStyle {
                        color: Some(Color(1.0, 1.0, 1.0, 0.55)),
                        font_size: TextUnit::Sp(36.0),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            );
        },
    );
}

/// A ball of fire flying around the room, with a halo that faces the
/// camera wherever it looks from.
#[composable(no_skip)]
pub(super) fn FireBall(room: RoomState, shape: RoomShape, (sphere, plane): (Arc<Mesh>, Arc<Mesh>)) {
    let time = rememberMutableStateOf(|| 0.0f32);
    Ticking(room.motion.get(), move |seconds| {
        time.update(|now| *now += seconds);
    });
    let radius = shape.width.min(shape.height) * 0.07;
    Box(
        Modifier::empty()
            .required_size(FIRE_CONTENT)
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&sphere),
                transform: Mat4::scaling(Vec3::new(radius, radius, radius))
                    .then(&Mat4::rotation_y(time.get() * 0.6))
                    .then(&Mat4::translation(fire_center(&shape, time.get()))),
                material: Material::Emissive,
                casts_shadow: false,
                solid: false,
            }),
        BoxSpec::default(),
        move || Flames(time),
    );
    Box(
        Modifier::empty()
            .required_size(HALO_CONTENT)
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&plane),
                transform: facing(
                    room.eye_camera(&shape),
                    fire_center(&shape, time.get()),
                    radius * HALO_PER_RADIUS,
                ),
                material: Material::Glow,
                casts_shadow: false,
                solid: false,
            })
            .draw_behind(|scope| {
                let size = scope.size();
                scope.draw_rect(Brush::radial_gradient_stops(
                    vec![
                        (0.0, Color(1.0, 0.8, 0.4, 1.0)),
                        (0.55, Color(1.0, 0.55, 0.15, 0.85)),
                        (0.75, Color(1.0, 0.35, 0.05, 0.35)),
                        (1.0, Color(0.8, 0.15, 0.0, 0.0)),
                    ],
                    Point::new(size.width * 0.5, size.height * 0.5),
                    size.width.min(size.height) * 0.5,
                    TileMode::Clamp,
                ));
            }),
        BoxSpec::default(),
        || {},
    );
}

/// Where the fire is `time` seconds after it set off: around the middle of
/// the room, rising and falling as it goes.
fn fire_center(shape: &RoomShape, time: f32) -> Vec3 {
    let angle = time * ORBIT_PER_SECOND;
    Vec3::new(
        shape.width * (0.5 + 0.3 * angle.cos()),
        shape.height * (0.4 + 0.08 * (2.0 * angle).sin()),
        shape.depth * (0.55 + 0.22 * angle.sin()),
    )
}

/// The unit plane as a square of `side` about `center`, turned to face
/// `camera` the right way up.
fn facing(camera: Camera, center: Vec3, side: f32) -> Mat4 {
    let rows = camera.view.rows();
    let axis = |row: [f32; 4]| Vec3::new(row[0], row[1], row[2]);
    let (right, down) = (axis(rows[0]) * side, axis(rows[1]) * side);
    Mat4::from_axes(
        right,
        down,
        axis(rows[2]),
        center - right * 0.5 - down * 0.5,
    )
}

/// The fire's surface: flames that rise and swirl over the whole ball.
#[composable(no_skip)]
fn Flames(time: MutableState<f32>) {
    Box(
        Modifier::empty()
            .fill_max_size()
            .background(Color::BLACK)
            .graphics_layer(move || GraphicsLayer {
                render_effect: Some(flames(time.get())),
                ..GraphicsLayer::default()
            }),
        BoxSpec::default(),
        || {},
    );
}

fn flames(time: f32) -> RenderEffect {
    let mut shader = RuntimeShader::from_shared_source(flames_wgsl());
    shader.set_position_independent(true);
    shader.set_float(0, time);
    RenderEffect::runtime_shader(shader)
}

/// Fire over a ball's content: its point is taken on the unit sphere, so
/// the flames meet at the seam and do not pinch at the poles, and they rise
/// toward the top.
fn flames_wgsl() -> Arc<str> {
    static SOURCE: OnceLock<Arc<str>> = OnceLock::new();
    Arc::clone(SOURCE
        .get_or_init(|| {
            Arc::<str>::from(format!(
                r"{RUNTIME_SHADER_PRELUDE_WGSL}{WGSL_HELPERS}

fn hash3(p: vec3<f32>) -> f32 {{
    return fract(sin(dot(p, vec3<f32>(127.1, 311.7, 74.7))) * 43758.5453);
}}

fn noise3(p: vec3<f32>) -> f32 {{
    let i = floor(p);
    let f = fract(p);
    let s = f * f * (3.0 - 2.0 * f);
    let a = mix(hash3(i), hash3(i + vec3<f32>(1.0, 0.0, 0.0)), s.x);
    let b = mix(hash3(i + vec3<f32>(0.0, 1.0, 0.0)), hash3(i + vec3<f32>(1.0, 1.0, 0.0)), s.x);
    let c = mix(hash3(i + vec3<f32>(0.0, 0.0, 1.0)), hash3(i + vec3<f32>(1.0, 0.0, 1.0)), s.x);
    let d = mix(hash3(i + vec3<f32>(0.0, 1.0, 1.0)), hash3(i + vec3<f32>(1.0, 1.0, 1.0)), s.x);
    return mix(mix(a, b, s.y), mix(c, d, s.y), s.z);
}}

fn fbm3(start: vec3<f32>) -> f32 {{
    var p = start;
    var sum = 0.0;
    var weight = 0.5;
    for (var octave = 0; octave < 5; octave++) {{
        sum += noise3(p) * weight;
        p = p * 2.03 + vec3<f32>(1.7, 9.2, 3.1);
        weight *= 0.5;
    }}
    return sum;
}}

fn fire_colour(heat: f32) -> vec3<f32> {{
    let ember = vec3<f32>(0.25, 0.02, 0.0);
    let red = vec3<f32>(0.95, 0.18, 0.02);
    let orange = vec3<f32>(1.0, 0.55, 0.08);
    let yellow = vec3<f32>(1.0, 0.9, 0.45);
    var colour = mix(ember, red, smoothstep(0.0, 0.35, heat));
    colour = mix(colour, orange, smoothstep(0.3, 0.6, heat));
    return mix(colour, yellow, smoothstep(0.6, 0.95, heat));
}}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {{
    let tex_size = vec2<f32>(textureDimensions(input_texture));
    let rect = vec4<f32>(get_float(248u), get_float(249u), get_float(250u), get_float(251u));
    let uv = clamp((input.uv * tex_size - rect.xy) / max(rect.zw, vec2<f32>(1.0)), vec2<f32>(0.0), vec2<f32>(1.0));
    let time = get_float(0u);
    let around = (uv.x - 0.5) * 6.2831853;
    let polar = uv.y * 3.1415927;
    let point = vec3<f32>(sin(polar) * sin(around), -cos(polar), -sin(polar) * cos(around));
    // The field flows down through the ball, so the flames rise.
    let flow = vec3<f32>(0.0, time * 1.1, 0.0);
    let warp = vec3<f32>(
        fbm3(point * 2.2 + flow),
        fbm3(point * 2.2 + flow + vec3<f32>(5.2, 1.3, 2.7)),
        fbm3(point * 2.2 - flow * 0.5 + vec3<f32>(1.7, 9.2, 3.1)),
    );
    let heat = clamp(fbm3(point * 3.2 + warp * 1.8 + flow * 1.6) * 1.6 - 0.25, 0.0, 1.0);
    return vec4<f32>(fire_colour(heat), 1.0);
}}
"
            ))
        }))
}
