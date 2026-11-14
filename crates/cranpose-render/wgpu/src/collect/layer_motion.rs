// The raster scales of animated layers, held from frame to frame while
// their content stays.

use cranpose_core::{NodeId, RecordRef, collections::map::HashMap};
use cranpose_render_common::scene_store::{LayerRef, SceneStore};

pub(super) const ANIMATED_RASTER_STEPS_PER_OCTAVE: f32 = 8.0;

#[derive(Default)]
pub(crate) struct LayerMotion {
    previous: HashMap<NodeId, Motion>,
    current: HashMap<NodeId, Motion>,
}

#[derive(Clone, Copy)]
struct Motion {
    record: RecordRef,
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

    pub(super) fn record_scale(&mut self, layer: LayerRef<'_>, raster: f32, content_hash: u64) {
        let Some(node_id) = layer.node_id() else {
            return;
        };
        self.current.insert(
            node_id,
            Motion {
                record: layer.store().record_ref(layer.id()),
                content_hash,
                raster,
            },
        );
    }

    // Ends a frame whose walk left layers of `store` out: their scales stay
    // while their records stand.
    pub(super) fn end_culled_frame(&mut self, store: &SceneStore) {
        self.previous
            .retain(|_, motion| store.resolve(motion.record).is_some());
        self.previous.extend(self.current.drain());
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
