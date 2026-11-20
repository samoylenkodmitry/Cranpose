// A lit torus that a controller's ray carries: Cranpose's 3D in a subspace. The torus is the OBJ
// mesh of the desktop demo, with a striped texture drawn by a composable. It turns about its
// own axis in each frame, which the scene draws without composing anything.

use std::{f32::consts::FRAC_PI_2, sync::Arc};

use cranpose::{
    LaunchedEffect,
    prelude::*,
    space::{Light, Mat4, Material, Mesh, MeshSurface, Vec3},
};
use cranpose_xr::{SpatialVolume, SubspaceModifier};

// The OBJ file of the torus: radius 1 about the y axis, tube radius 0.4.
static TORUS_OBJ: &str = include_str!("../../desktop-demo/assets/torus.obj");

// The box of the volume in dp, which are millimeters, and the torus's outer radius in them: the
// ring's 1 plus the tube's 0.4, scaled by `SCALE`.
const BOX: f32 = 300.0;
const SCALE: f32 = 100.0;
// The texture's size in pixels. It wraps once around the ring and once around the tube.
const TEXTURE: Size = Size {
    width: 512.0,
    height: 128.0,
};
// How fast the torus turns, in radians a second.
const TURN_PER_SECOND: f32 = 0.9;
// How far the torus leans toward the viewer: its hole faces the viewer at a quarter turn.
const LEAN: f32 = FRAC_PI_2 - 0.45;
// The longest step of a frame, in seconds, so that a late frame does not jump the turn.
const LONGEST_STEP: f32 = 0.1;

// The torus in a volume at `modifier`'s place. The user carries it by the handle under its box.
#[composable]
pub(crate) fn TorusVolume(modifier: SubspaceModifier) {
    let torus = remember(|| {
        Mesh::from_obj(TORUS_OBJ)
            .map_err(|error| log::error!("xr demo: no torus: {error}"))
            .ok()
            .map(Arc::new)
    })
    .with(Clone::clone);
    let Some(torus) = torus else {
        return;
    };
    let turn = rememberMutableStateOf(|| 0.0_f32);
    LaunchedEffect((), async move |scope| {
        let mut last = scope.with_frame_nanos(|now| now).await;
        loop {
            let now = scope.with_frame_nanos(|now| now).await;
            let step = (now.saturating_sub(last) as f32 * 1.0e-9).min(LONGEST_STEP);
            turn.update(|angle| *angle = (*angle + step * TURN_PER_SECOND) % std::f32::consts::TAU);
            last = now;
        }
    });
    SpatialVolume(modifier.size(BOX, BOX).depth(BOX).movable(), move |scope| {
        // The light shines from the upper left, over the viewer's shoulder
        scope.light(Light {
            direction: Vec3::new(0.45, 0.6, 0.65),
            ambient: 0.3,
            shadows: false,
            size: 0.0,
        });
        Torus(Arc::clone(&torus), turn);
    });
}

// The torus's node: its content is the stripes that the mesh shows, and the mesh follows the turn.
#[composable]
fn Torus(mesh: Arc<Mesh>, turn: MutableState<f32>) {
    Box(
        Modifier::empty()
            .required_size(TEXTURE)
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&mesh),
                transform: Mat4::scaling(Vec3::new(SCALE, SCALE, SCALE))
                    .then(&Mat4::rotation_y(turn.get()))
                    .then(&Mat4::rotation_x(LEAN)),
                material: Material::Lit,
                casts_shadow: false,
                solid: true,
            })
            .draw_behind(|draw| {
                let size = draw.size();
                draw.draw_rect(Brush::solid(Color(0.95, 0.55, 0.15, 1.0)));
                let stripes = 16;
                for stripe in (0..stripes).step_by(2) {
                    let band = Rect {
                        x: size.width * stripe as f32 / stripes as f32,
                        y: 0.0,
                        width: size.width / stripes as f32,
                        height: size.height,
                    };
                    draw.draw_rect_at(band, Brush::solid(Color(0.15, 0.4, 0.85, 1.0)));
                }
            }),
        BoxSpec::default(),
        || {},
    );
}
