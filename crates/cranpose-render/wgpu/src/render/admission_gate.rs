//! A layer cache entry waits behind a gate that admits it once its content
//! has held still long enough to pay for keeping it.

use cranpose_core::{NodeId, collections::map::HashMap};
use cranpose_render_common::{graph::DrawCommandId, raster_cache::LayerRasterCacheKey};

use crate::layer_cache::LayerCache;

const MAX_ADMISSION_PATIENCE: u32 = 16;
// The frames a layer that can draw in place holds its content still before
// its surface is kept.
const IN_PLACE_PATIENCE: u32 = MAX_ADMISSION_PATIENCE;
// The longest that wait grows after surfaces kept for such a layer went
// unread: about four seconds at 60 Hz.
const IN_PLACE_MAX_PATIENCE: u32 = 16 * IN_PLACE_PATIENCE;

// The reads a kept surface needs to pay for itself. Keeping it costs a
// copy or a draw into a surface of its own, and the texture it holds; read
// once and then replaced -- a cell whose size holds two or three frames of
// a relayout -- it cost about what drawing it again would have, and its
// texture churned the pool.
const READS_TO_PAY: u32 = 2;

enum AdmissionCost {
    Pin,
    // A kept surface costs a copy. `patience` is the frames the content
    // holds still before its surface is kept: it doubles, up to `ceiling`,
    // whenever a kept surface was replaced before it was read
    // [`READS_TO_PAY`] times, and halves, down to `floor`, when one was.
    Copy {
        patience: u32,
        floor: u32,
        ceiling: u32,
    },
}

pub(crate) struct AdmissionGate {
    key: LayerRasterCacheKey,
    run: u32,
    cost: AdmissionCost,
    admitted: bool,
    // Frames the kept surface of the current key has been read.
    reads: u32,
    seen: bool,
}

impl AdmissionGate {
    pub(crate) fn pinned(key: LayerRasterCacheKey) -> Self {
        Self::with_cost(key, AdmissionCost::Pin)
    }

    // A gate for a surface kept by copying it out of the pass it was drawn
    // in: kept once its content has held still for a second frame.
    pub(crate) fn copied(key: LayerRasterCacheKey) -> Self {
        Self::with_cost(
            key,
            AdmissionCost::Copy {
                patience: 1,
                floor: 1,
                ceiling: MAX_ADMISSION_PATIENCE,
            },
        )
    }

    // A gate for a surface rendered into a texture of its own, kept from
    // its first sight: still content reads it back the next frame.
    pub(crate) fn rendered(key: LayerRasterCacheKey) -> Self {
        Self::with_cost(
            key,
            AdmissionCost::Copy {
                patience: 0,
                floor: 0,
                ceiling: MAX_ADMISSION_PATIENCE,
            },
        )
    }

    // A gate for a member of a shared atlas, whose kept surface is copied
    // out of it: kept once its content has held still for two frames after
    // its first. A screen's second frame repeats its first, since a
    // frame-clock animation takes its start time there, so content seen
    // alike on both has not yet shown whether it moves. Kept on its first
    // or second sight, every member of a starting screen took a copy and a
    // texture of its own that the next frame left unread wherever the
    // member animated: the Mate 20 X gauntlet copied 96 cards at tier 12
    // for the 14 that held still, and its GPU allocator held 3 to 8 MB
    // more for as long as it ran.
    pub(crate) fn copied_out_of_atlas(key: LayerRasterCacheKey) -> Self {
        Self::with_cost(
            key,
            AdmissionCost::Copy {
                patience: 2,
                floor: 1,
                ceiling: MAX_ADMISSION_PATIENCE,
            },
        )
    }

    // A gate for a surface its layer can do without by drawing in place:
    // the surface is kept only once its content has held still for
    // `IN_PLACE_PATIENCE` frames. Drawing in place meanwhile costs nothing
    // a surface would save, and a shorter hold -- a relayout pausing at the
    // turn of its motion -- would keep surfaces read for a frame or two.
    // A pause longer than that at every turn keeps surfaces nothing reads,
    // so each one that goes unread doubles the wait, up to
    // `IN_PLACE_MAX_PATIENCE`.
    pub(crate) fn drawn_in_place(key: LayerRasterCacheKey) -> Self {
        Self::with_cost(
            key,
            AdmissionCost::Copy {
                patience: IN_PLACE_PATIENCE,
                floor: IN_PLACE_PATIENCE,
                ceiling: IN_PLACE_MAX_PATIENCE,
            },
        )
    }

    fn with_cost(key: LayerRasterCacheKey, cost: AdmissionCost) -> Self {
        Self {
            key,
            run: 1,
            cost,
            admitted: false,
            reads: 0,
            seen: true,
        }
    }

    pub(crate) fn observe(&mut self, key: LayerRasterCacheKey) -> Option<LayerRasterCacheKey> {
        self.seen = true;
        if self.key == key {
            self.run = self.run.saturating_add(1);
            return None;
        }
        if let AdmissionCost::Copy {
            patience, ceiling, ..
        } = &mut self.cost
            && self.admitted
            && self.reads < READS_TO_PAY
        {
            *patience = (*patience * 2).clamp(1, *ceiling);
        }
        let dead = self.dead_entry();
        self.admitted = false;
        self.reads = 0;
        self.key = key;
        self.run = 1;
        dead
    }

    pub(crate) fn key(&self) -> LayerRasterCacheKey {
        self.key
    }

    pub(crate) fn dead_entry(&self) -> Option<LayerRasterCacheKey> {
        let dead = match self.cost {
            AdmissionCost::Pin => self.admitted,
            AdmissionCost::Copy { .. } => self.admitted && self.reads == 0,
        };
        dead.then_some(self.key)
    }

    pub(crate) fn admits(&self) -> bool {
        match self.cost {
            AdmissionCost::Pin => true,
            AdmissionCost::Copy { patience, .. } => self.run > patience,
        }
    }

    pub(crate) fn admitted(&mut self) {
        self.admitted = true;
        self.reads = 0;
    }

    pub(crate) fn hit(&mut self, key: LayerRasterCacheKey) {
        self.observe(key);
        self.reads = self.reads.saturating_add(1);
        // A kept surface that paid for itself halves the wait, once: content
        // that holds still at the turns of a motion and churns between them
        // keeps a wait the churn does not get past.
        if let AdmissionCost::Copy {
            patience, floor, ..
        } = &mut self.cost
            && self.reads == READS_TO_PAY
        {
            *patience = (*patience / 2).max(*floor);
        }
    }

    pub(crate) fn end_frame(&mut self) -> bool {
        std::mem::take(&mut self.seen)
    }
}

// The gate a node keeps for one of its kept surfaces.
#[derive(Clone, Copy)]
pub(crate) enum NodeGate {
    Backdrop,
    Effect,
    Source,
}

#[derive(Default)]
struct NodeGates {
    backdrop: Option<AdmissionGate>,
    effect: Option<AdmissionGate>,
    source: Option<AdmissionGate>,
}

impl NodeGates {
    fn slot(&mut self, kind: NodeGate) -> &mut Option<AdmissionGate> {
        match kind {
            NodeGate::Backdrop => &mut self.backdrop,
            NodeGate::Effect => &mut self.effect,
            NodeGate::Source => &mut self.source,
        }
    }

    fn slots(&mut self) -> [&mut Option<AdmissionGate>; 3] {
        [&mut self.backdrop, &mut self.effect, &mut self.source]
    }
}

// The renderer's admission gates: those of the nodes' kept surfaces and
// those of the fill draws kept as prefixes.
#[derive(Default)]
pub(crate) struct AdmissionGates {
    nodes: HashMap<NodeId, NodeGates>,
    pub(crate) fills: HashMap<DrawCommandId, AdmissionGate>,
}

impl AdmissionGates {
    pub(crate) fn node(&mut self, node_id: NodeId, kind: NodeGate) -> Option<&mut AdmissionGate> {
        self.nodes.get_mut(&node_id)?.slot(kind).as_mut()
    }

    // Observes `key` with the node's gate of `kind`, started by `start` when
    // the node has none, and drops the cache entry a superseded key leaves.
    // Whether the gate admits.
    pub(crate) fn observe_node(
        &mut self,
        (node_id, kind): (NodeId, NodeGate),
        key: LayerRasterCacheKey,
        start: fn(LayerRasterCacheKey) -> AdmissionGate,
        layer_cache: &mut LayerCache,
    ) -> bool {
        let slot = self.nodes.entry(node_id).or_default().slot(kind);
        match slot {
            Some(gate) => {
                if let Some(dead) = gate.observe(key) {
                    layer_cache.remove(&dead);
                }
                gate.admits()
            }
            None => slot.insert(start(key)).admits(),
        }
    }

    // Ends a frame: a gate no draw reached goes, and the cache entry it held.
    pub(crate) fn end_frame(&mut self, layer_cache: &mut LayerCache) {
        let mut retire = |gate: &mut AdmissionGate| {
            let seen = gate.end_frame();
            if !seen && let Some(dead) = gate.dead_entry() {
                layer_cache.remove(&dead);
            }
            seen
        };
        self.fills.retain(|_, gate| retire(gate));
        self.nodes.retain(|_, gates| {
            let mut any = false;
            for slot in gates.slots() {
                if slot.as_mut().is_some_and(|gate| !retire(gate)) {
                    *slot = None;
                }
                any |= slot.is_some();
            }
            any
        });
    }
}

#[cfg(test)]
#[path = "../tests/admission_gate_tests.rs"]
mod tests;
