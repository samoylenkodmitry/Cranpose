// What has run down gathers along the foot of the wall: a puddle that rises
// at the end of the melt, deeper where a runner has arrived. It stands in
// the stage's own space, below the sheet, and pours in the same chocolate.

use std::{rc::Rc, sync::Arc};

use cranpose_core::remember;
use cranpose_ui::{
    composable,
    space::{Mat4, Mesh, MeshSurface, Vec3},
    Box, BoxSpec, Brush, Canvas, Modifier, Point, Rect, Size,
};

use super::{
    chocolate::{chocolate_layer, poured, Pour},
    drips::DRIP_INK,
    melting::Melting,
    walk::ease,
};

// In units.
const PUDDLE_HEIGHT: f32 = 110.0;
const RIM_STEP: f32 = 14.0;

#[composable(no_skip)]
pub(super) fn Puddle(melting: Melting, (landings, depth): (Rc<[f32]>, f32), density: f32) {
    let u = melting.u;
    let size = Size::new(melting.size.width, PUDDLE_HEIGHT * u);
    let offset = Point::new(0.0, melting.size.height - size.height);
    let pour = Pour {
        size,
        offset,
        head_scale: 1.0,
        copy_scale: 1.0,
    };
    let plane = remember(|| Arc::new(Mesh::plane())).with(Arc::clone);
    Box(
        Modifier::empty()
            .required_size(size)
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&plane),
                transform: Mat4::scaling(Vec3::new(size.width, size.height, 1.0))
                    .then(&Mat4::translation(Vec3::new(offset.x, offset.y, depth))),
                material: poured(),
                casts_shadow: false,
                solid: false,
            }),
        BoxSpec::default(),
        move || {
            let landings = Rc::clone(&landings);
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .graphics_layer(move || chocolate_layer(pour, melting, density)),
                BoxSpec::default(),
                move || {
                    let landings = Rc::clone(&landings);
                    PuddleInk(size, melting, landings);
                },
            );
        },
    );
}

#[composable(no_skip)]
fn PuddleInk(size: Size, melting: Melting, landings: Rc<[f32]>) {
    let u = melting.u;
    Canvas(Modifier::empty().fill_max_size(), move |scope| {
        let progress = melting.progress();
        let pool = 30.0 * u * ease(0.78, 1.0, progress);
        let arrived = 46.0 * u * ease(0.6, 1.0, progress);
        if pool + arrived <= 0.0 {
            return;
        }
        let ink = Brush::solid(DRIP_INK);
        let step = RIM_STEP * u;
        let columns = (size.width / step).ceil() as usize + 1;
        for column in 0..=columns {
            let x = column as f32 * step;
            let amount = pool
                + landings
                    .iter()
                    .map(|&landing| {
                        let spread = (34.0 + 110.0 * ease(0.6, 1.0, progress)) * u;
                        let d = (x - landing) / spread;
                        arrived * (-d * d * 0.5).exp()
                    })
                    .sum::<f32>();
            let along = x / u;
            let depth = 66.0
                * u
                * (1.0 - (-amount / (66.0 * u)).exp())
                * (1.0 + 0.16 * (along * 0.0173 + 1.0).sin() + 0.10 * (along * 0.0431 + 0.4).sin());
            if depth < 0.5 {
                continue;
            }
            let top = size.height + 5.0 * u - depth;
            scope.draw_rect_at(
                Rect {
                    x: x - step * 0.5,
                    y: top + step * 0.5,
                    width: step,
                    height: size.height - top,
                },
                ink.clone(),
            );
            scope.draw_circle(ink.clone(), Point::new(x, top + step * 0.5), step * 0.62);
        }
    });
}
