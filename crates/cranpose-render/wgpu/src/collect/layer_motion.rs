// The raster scales of animated layers, held from frame to frame while
// their content stays.

use cranpose_core::{NodeId, collections::map::HashMap};

pub(super) const ANIMATED_RASTER_STEPS_PER_OCTAVE: f32 = 8.0;

#[derive(Default)]
pub(crate) struct LayerMotion {
    previous: HashMap<NodeId, Motion>,
    current: HashMap<NodeId, Motion>,
}

#[derive(Clone, Copy)]
struct Motion {
    content_hash: u64,
    raster: f32,
}

impl LayerMotion {
    pub(super) fn retained_scale(
        &self,
        node_id: Option<NodeId>,
        scale: f32,
        content_hash: u64,
    ) -> f32 {
        node_id
            .and_then(|id| self.previous.get(&id))
            .filter(|previous| previous.content_hash == content_hash)
            .filter(|_| scale.is_finite() && scale > 0.0)
            .map_or(scale, |previous| held_raster_scale(previous.raster, scale))
    }

    pub(super) fn record_scale(&mut self, node_id: Option<NodeId>, raster: f32, content_hash: u64) {
        let Some(node_id) = node_id else {
            return;
        };
        self.current.insert(
            node_id,
            Motion {
                content_hash,
                raster,
            },
        );
    }

    pub(crate) fn end_frame(&mut self) {
        std::mem::swap(&mut self.previous, &mut self.current);
        self.current.clear();
    }
}

fn held_raster_scale(held: f32, scale: f32) -> f32 {
    if held * (1.0 / ANIMATED_RASTER_STEPS_PER_OCTAVE).exp2() >= scale && held <= scale * 2.0 {
        held
    } else {
        animated_raster_scale(scale)
    }
}

pub(crate) fn animated_raster_scale(scale: f32) -> f32 {
    let step =
        (scale.log2() * ANIMATED_RASTER_STEPS_PER_OCTAVE).ceil() / ANIMATED_RASTER_STEPS_PER_OCTAVE;
    step.exp2().max(scale)
}
