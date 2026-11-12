// What the shell gives back at rest: the room a burst took in the lists its
// frames fill, such as a first frame's, which names every node, and in its
// context's, its scenes' and its renderers' storage.

use std::fmt::Debug;

use cranpose_render_common::{RenderScene, Renderer};

use crate::AppShell;

// The nodes each list keeps room for at rest.
const LIST_ROOM_KEPT: usize = 64;

impl<R> AppShell<R>
where
    R: Renderer,
    R::Error: Debug,
{
    // Out of line: few frames end at rest.
    #[cold]
    #[inline(never)]
    pub(crate) fn trim(&mut self) {
        self.app.app_context.trim();
        self.routing_scratch.trim();
        self.pending_dirty_nodes.shrink_to(LIST_ROOM_KEPT);
        self.pending_layer_property_nodes.shrink_to(LIST_ROOM_KEPT);
        let geometry = &mut self.geometry_scene_nodes;
        geometry.reshaped.shrink_to(LIST_ROOM_KEPT);
        geometry.moved.shrink_to(LIST_ROOM_KEPT);
        geometry.unplaced.shrink_to(LIST_ROOM_KEPT);
        self.app.left_nodes.shrink_to(LIST_ROOM_KEPT);
        for surface in &mut self.surfaces {
            for list in [
                &mut surface.scoped_layout_scene_nodes,
                &mut surface.scoped_moved_scene_nodes,
                &mut surface.scoped_draw_nodes,
                &mut surface.scoped_layer_property_nodes,
                &mut surface.structural_scene_nodes,
            ] {
                list.shrink_to(LIST_ROOM_KEPT);
            }
            surface.scene_updates.shrink_to(LIST_ROOM_KEPT);
            surface.renderer.scene_mut().trim();
            surface.renderer.trim();
        }
    }
}
