use cranpose_core::{MemoryApplier, NodeId};
use cranpose_render_common::{NodeUpdate, Renderer, SceneUpdates, graph_scene::Scene};
use cranpose_ui::Size;

#[derive(Default)]
pub(crate) struct GraphRenderer {
    pub(crate) scene: Scene,
    pub(crate) rebuilds: usize,
    pub(crate) updates: usize,
    pub(crate) dirty_ids: usize,
}

impl Renderer for GraphRenderer {
    type Scene = Scene;
    type Error = ();

    fn scene(&self) -> &Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Scene {
        &mut self.scene
    }

    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        _: Size,
    ) -> Result<(), ()> {
        self.rebuilds += 1;
        self.scene.rebuild_from_applier(applier, root);
        self.scene.root_layer().map(|_| ()).ok_or(())
    }

    fn update_scene_from_applier(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        _: Size,
        updates: SceneUpdates<'_>,
    ) -> Result<(), ()> {
        self.updates += 1;
        self.dirty_ids += updates
            .iter()
            .filter(|&&(_, update)| update != NodeUpdate::Moved)
            .count();
        if !self.scene.update_from_applier(applier, root, updates) {
            self.rebuilds += 1;
        }
        self.scene.root_layer().map(|_| ()).ok_or(())
    }
}
