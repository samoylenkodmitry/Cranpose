// Which draw observations the shell keeps: those of the nodes some surface
// draws. A node that stops drawing keeps no observation that would ask to
// redraw it.

use std::fmt::Debug;

use cranpose_core::NodeId;
use cranpose_render_common::{RenderScene, Renderer};

use crate::{AppShell, surface::RootSurface};

impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
    // Retires the draw observations of the nodes no surface draws any more:
    // each frame those whose records the surfaces' scenes dropped, and after
    // a surface could not tell, those of every node no surface draws.
    pub(super) fn retire_draw_observations(&mut self) {
        let left = &mut self.app.left_nodes;
        left.clear();
        let mut known = true;
        for surface in &mut self.surfaces {
            known &= surface.renderer.scene_mut().read_left_nodes(left);
        }
        if !known {
            retire_every_undrawn(&self.surfaces);
        } else if !left.is_empty() {
            retire_undrawn(&self.surfaces, left);
        }
    }
}

fn drawn<R: Renderer>(surfaces: &[RootSurface<R>], node: NodeId) -> bool {
    surfaces
        .iter()
        .any(|surface| surface.renderer.scene().draws(node))
}

// Retires the draw observations of the nodes in `left` no surface draws.
// Out of line: most frames drop no record.
#[inline(never)]
fn retire_undrawn<R: Renderer>(surfaces: &[RootSurface<R>], left: &mut Vec<NodeId>) {
    left.retain(|&node| !drawn(surfaces, node));
    if left.is_empty() {
        return;
    }
    left.sort_unstable();
    cranpose_ui::retire_draw_observations(|node| left.binary_search(&node).is_ok());
}

// Retires the draw observations of every node no surface draws, once a
// surface could not tell which records it dropped.
#[cold]
#[inline(never)]
fn retire_every_undrawn<R: Renderer>(surfaces: &[RootSurface<R>]) {
    cranpose_ui::retire_draw_observations(|node| !drawn(surfaces, node));
}
