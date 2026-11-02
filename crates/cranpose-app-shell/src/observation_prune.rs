// When the shell drops the draw observations of nodes no surface's scene
// holds any more.

use cranpose_core::NodeId;
use cranpose_render_common::{RenderScene, Renderer};

use crate::surface::RootSurface;

// Rebuilt frames between prunes of stale draw observations: once a second
// at 60 Hz.
pub(crate) const OBSERVATION_PRUNE_INTERVAL: u32 = 60;

// The rebuilt frames since the last prune, and the nodes the scenes held at
// it, sorted, kept for their storage.
#[derive(Default)]
pub(crate) struct ObservationPrune {
    pub(crate) frames: u32,
    nodes: Vec<NodeId>,
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
        let nodes = &mut self.nodes;
        nodes.clear();
        let mut any_scene = false;
        for surface in surfaces {
            any_scene |= surface
                .renderer
                .scene()
                .visit_retained_visual_observation_nodes(&mut |node| nodes.push(node));
        }
        if any_scene {
            nodes.sort_unstable();
            cranpose_ui::prune_draw_observations_to_nodes(nodes);
        }
    }
}
