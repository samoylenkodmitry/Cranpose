// When the shell drops the draw observations of nodes no surface's scene
// holds any more.

use cranpose_core::{NodeId, collections::map::HashSet};
use cranpose_render_common::{RenderScene, Renderer};

use crate::surface::RootSurface;

// Rebuilt frames between prunes of stale draw observations: once a second
// at 60 Hz.
pub(crate) const OBSERVATION_PRUNE_INTERVAL: u32 = 60;

// The rebuilt frames since the last prune, and how many nodes the scenes
// held at it, which the next prune takes room for at once.
#[derive(Default)]
pub(crate) struct ObservationPrune {
    pub(crate) frames: u32,
    held: usize,
}

impl ObservationPrune {
    // Drops the draw observations of nodes no scene of `surfaces` holds any
    // more, once every `OBSERVATION_PRUNE_INTERVAL` rebuilt frames: finding
    // them walks every node of every scene, and an observation left behind
    // in between costs no more than a redraw request for a node nothing
    // draws.
    pub(crate) fn after_rebuild<R: Renderer>(&mut self, surfaces: &[RootSurface<R>]) {
        self.frames += 1;
        if self.frames < OBSERVATION_PRUNE_INTERVAL {
            return;
        }
        self.frames = 0;
        let mut nodes: HashSet<NodeId> =
            HashSet::with_capacity_and_hasher(self.held, Default::default());
        let mut any_scene = false;
        for surface in surfaces {
            any_scene |= surface
                .renderer
                .scene()
                .visit_retained_visual_observation_nodes(&mut |node| {
                    nodes.insert(node);
                });
        }
        self.held = nodes.len();
        if any_scene {
            cranpose_ui::prune_draw_observations_to_nodes(&nodes);
        }
    }
}
