//! How a context gives back at rest the room its node lists took.

use super::AppContext;

impl AppContext {
    /// Gives back the room the context's lists of nodes hold past their
    /// nodes: the room a burst took, such as a first frame's, which names
    /// every node. Its shell calls it at rest, when no frame waits, as a
    /// frame would take the room again.
    pub fn trim(&self) {
        let state = &self.state;
        for set in [
            &state.layout_repasses,
            &state.measure_repasses,
            &state.draw_repasses,
            &state.layer_property_repasses,
            &state.modifier_slice_repasses,
            &state.z_order_changes,
        ] {
            set.borrow_mut().trim();
        }
        state.geometry_scene_nodes.borrow_mut().trim();
        state.moved_scene_nodes.borrow_mut().trim();
        self.placement_pass().trim();
    }
}
