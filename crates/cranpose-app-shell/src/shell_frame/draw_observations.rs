// Which draw observations the shell keeps: those of the nodes some surface
// draws. A node that stops drawing keeps no observation that would ask to
// redraw it.

use std::fmt::Debug;

use cranpose_core::NodeId;
use cranpose_render_common::{DrawOwnerChange, RenderScene, Renderer, undrawn_nodes};

use crate::AppShell;

// Frames between full prunes of the draw observations while a surface
// cannot tell which nodes it stopped drawing: once a second at 60 Hz. A full
// prune walks every node of every scene, and an observation left behind in
// between costs no more than a redraw request for a node nothing draws.
pub(crate) const OBSERVATION_PRUNE_INTERVAL: u32 = 60;

#[derive(Default)]
pub(crate) struct DrawObservations {
    // The nodes the surfaces started or stopped drawing in a frame, and
    // room for those that left every surface and those that joined one.
    changes: Vec<DrawOwnerChange>,
    left: Vec<NodeId>,
    joined: Vec<NodeId>,
    // Frames since a surface could not tell which nodes it stopped drawing,
    // until a full prune catches up.
    lost_for: Option<u32>,
}

impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
    // Retires the draw observations of the nodes no surface draws any more:
    // each frame those the surfaces' scenes report, and after a surface
    // could not tell, all of them in a full prune.
    pub(super) fn retire_draw_observations(&mut self) {
        let observations = &mut self.app.draw_observations;
        let DrawObservations {
            changes,
            left,
            joined,
            lost_for,
        } = observations;
        changes.clear();
        let mut known = true;
        for surface in &mut self.surfaces {
            known &= surface
                .renderer
                .scene_mut()
                .read_draw_owner_changes(changes);
        }
        if !known {
            lost_for.get_or_insert(0);
        } else if !changes.is_empty() {
            retire_undrawn(changes, left, joined);
        }
        let Some(frames) = lost_for.as_mut() else {
            return;
        };
        *frames += 1;
        if *frames < OBSERVATION_PRUNE_INTERVAL {
            return;
        }
        *lost_for = None;
        self.prune_draw_observations();
    }

    // Retires the draw observations of every node no surface's scene holds:
    // the full prune after a surface could not tell its changes.
    #[cold]
    #[inline(never)]
    fn prune_draw_observations(&mut self) {
        let mut any_graph = false;
        for surface in &mut self.surfaces {
            any_graph |= surface
                .renderer
                .scene()
                .collect_retained_visual_observation_nodes(&mut surface.retained_visual_nodes);
        }
        if any_graph {
            let surfaces = &self.surfaces;
            cranpose_ui::retire_draw_observations(|node| {
                !surfaces
                    .iter()
                    .any(|surface| surface.retained_visual_nodes.contains(&node))
            });
        }
    }
}

// Retires the draw observations of the nodes `changes` say no scene draws
// any more. Out of line: most frames change no node's drawing.
#[inline(never)]
fn retire_undrawn(changes: &[DrawOwnerChange], left: &mut Vec<NodeId>, joined: &mut Vec<NodeId>) {
    undrawn_nodes(changes, left, joined);
    if !left.is_empty() {
        cranpose_ui::retire_draw_observations(|node| left.binary_search(&node).is_ok());
    }
}
