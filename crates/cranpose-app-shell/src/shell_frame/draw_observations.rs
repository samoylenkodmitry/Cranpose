// Which draw observations the shell keeps: those of the nodes some surface
// draws. A node that stops drawing keeps no observation that would ask to
// redraw it.

use std::fmt::Debug;

use cranpose_core::collections::map::HashSet;
use cranpose_render_common::{DrawOwnerChange, RenderScene, Renderer};

use crate::AppShell;

// Frames between full prunes of the draw observations while a surface
// cannot tell which nodes it stopped drawing: once a second at 60 Hz. A full
// prune walks every node of every scene, and an observation left behind in
// between costs no more than a redraw request for a node nothing draws.
pub(crate) const OBSERVATION_PRUNE_INTERVAL: u32 = 60;

#[derive(Default)]
pub(crate) struct DrawObservations {
    // The nodes the surfaces started or stopped drawing in a frame.
    changes: Vec<DrawOwnerChange>,
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
        let changes = &mut observations.changes;
        changes.clear();
        let mut known = true;
        for surface in &mut self.surfaces {
            known &= surface
                .renderer
                .scene_mut()
                .read_draw_owner_changes(changes);
        }
        if !known {
            observations.lost_for.get_or_insert(0);
        } else if !changes.is_empty() {
            // A node one surface left and another joined is drawn.
            changes.sort_unstable_by_key(|change| {
                (change.node(), matches!(change, DrawOwnerChange::Left(_)))
            });
            changes.dedup_by_key(|change| change.node());
            changes.retain(|change| matches!(change, DrawOwnerChange::Left(_)));
            if !changes.is_empty() {
                cranpose_ui::retire_draw_observations(|node| {
                    changes
                        .binary_search_by_key(&node, |change| change.node())
                        .is_ok()
                });
            }
        }
        let Some(frames) = observations.lost_for.as_mut() else {
            return;
        };
        *frames += 1;
        if *frames < OBSERVATION_PRUNE_INTERVAL {
            return;
        }
        observations.lost_for = None;
        let mut retained_visual_nodes = HashSet::default();
        let mut any_graph = false;
        for surface in &mut self.surfaces {
            any_graph |= surface
                .renderer
                .scene()
                .collect_retained_visual_observation_nodes(&mut surface.retained_visual_nodes);
            retained_visual_nodes.extend(surface.retained_visual_nodes.iter().copied());
        }
        if any_graph {
            cranpose_ui::retire_draw_observations(|node| !retained_visual_nodes.contains(&node));
        }
    }
}
