use std::rc::Rc;

use cranpose::prelude::*;

use super::rememberFrameSeconds;
use crate::data::{self, PALETTE, Particle, is_circle, wrap_unit};

#[composable]
pub fn ParticlesScreen(count: usize) {
    let particles: Rc<[Particle]> = remember(|| Rc::<[Particle]>::from(data::particles(count)))
        .with(|particles| particles.clone());
    let brushes: Rc<[Brush]> = remember(|| {
        PALETTE
            .iter()
            .map(|color| Brush::solid(Color(color.0, color.1, color.2, 0.85)))
            .collect::<Rc<[_]>>()
    })
    .with(|brushes| brushes.clone());
    let seconds = rememberFrameSeconds();
    Box(
        Modifier::empty()
            .fill_max_width()
            .weight(1.0)
            .background(Color::from_rgb_u8(0x0B, 0x10, 0x20))
            .draw_behind(move |scope| {
                // Read in the draw phase only: frames redraw, nothing recomposes.
                let t = seconds.get();
                let size = scope.size();
                for (index, particle) in particles.iter().enumerate() {
                    let x = wrap_unit(particle.x + particle.vx * t) * size.width;
                    let y = wrap_unit(particle.y + particle.vy * t) * size.height;
                    let brush = brushes[particle.color].clone();
                    if is_circle(index, count) {
                        scope.draw_circle(brush, Point { x, y }, particle.size);
                    } else {
                        scope.draw_round_rect_at(
                            Rect {
                                x,
                                y,
                                width: particle.size,
                                height: particle.size,
                            },
                            brush,
                            CornerRadii::uniform(3.0),
                        );
                    }
                }
            }),
        BoxSpec::default(),
        || {},
    );
}
