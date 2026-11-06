// The raster scales of animated layers, held from frame to frame while
// their content stays.

use cranpose_core::{NodeId, collections::map::HashMap};

pub(super) const ANIMATED_RASTER_STEPS_PER_OCTAVE: f32 = 8.0;

#[derive(Default)]
pub(crate) struct LayerMotion {
    previous: HashMap<NodeId, Motion>,
    current: HashMap<NodeId, Motion>,
    // The scales recorded, in order, while a content that a later collect
    // may take is collected, so that the layer whose content it takes
    // records again what its layers recorded then.
    log: Vec<(NodeId, Motion)>,
    // How many such contents are being collected, one inside another.
    logging: u32,
}

#[derive(Clone, Copy)]
pub(super) struct Motion {
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
        self.record(
            node_id,
            Motion {
                content_hash,
                raster,
            },
        );
    }

    fn record(&mut self, node_id: NodeId, motion: Motion) {
        self.current.insert(node_id, motion);
        if self.logging > 0 {
            self.log.push((node_id, motion));
        }
    }

    // Starts to log the scales that a content kept for later collects
    // records, and returns where its scales start in the log.
    pub(super) fn begin_log(&mut self) -> usize {
        self.logging += 1;
        self.log.len()
    }

    // The scales recorded since the `begin_log` that returned `from`.
    pub(super) fn end_log(&mut self, from: usize) -> Vec<(NodeId, Motion)> {
        self.logging -= 1;
        let scales = self.log[from..].to_vec();
        if self.logging == 0 {
            self.log.clear();
        }
        scales
    }

    // Records again `scales`, which the layers of a content collected in
    // an earlier frame recorded then.
    pub(super) fn record_again(&mut self, scales: &[(NodeId, Motion)]) {
        for &(node_id, motion) in scales {
            self.record(node_id, motion);
        }
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
