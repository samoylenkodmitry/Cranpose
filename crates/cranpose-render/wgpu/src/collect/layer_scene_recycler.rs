// The layer scenes of returned packets, which the next collect takes with
// their lists.

use super::LayerScene;
use crate::scene::{CompositorScene, SceneCapacityHint};

/// The layer scenes of returned packets, kept with their buffers for the
/// next collect. Boxed, as a layer's content is: a scene is some 540 bytes,
/// which every recycle, take and child push moved inline.
#[derive(Default)]
pub(crate) struct LayerSceneRecycler {
    #[expect(clippy::vec_box, reason = "scenes are recycled by box, so none moves")]
    scenes: Vec<Box<LayerScene>>,
}

impl LayerSceneRecycler {
    pub(crate) fn take(&mut self, capacity: SceneCapacityHint) -> Box<LayerScene> {
        match self.scenes.pop() {
            Some(mut scene) => {
                scene.has_backdrop = false;
                scene
            }
            None => Box::new(LayerScene::new(
                CompositorScene::with_capacity(capacity),
                Vec::new(),
            )),
        }
    }

    pub(crate) fn recycle(&mut self, mut scene: Box<LayerScene>) {
        while let Some(child) = scene.children.pop() {
            self.recycle(child.content);
        }
        scene.scene.settle_room();
        scene.scene.clear();
        scene.has_backdrop = false;
        scene.shown.clear();
        self.scenes.push(scene);
    }

    pub(crate) fn clear(&mut self) {
        self.scenes.clear();
    }

    // The bytes of room the lists of the kept scenes hold.
    pub(crate) fn kept_room(&self) -> usize {
        self.scenes
            .iter()
            .map(|scene| scene.scene.kept_room())
            .sum()
    }

    // The bytes of room kept by the root scene that the next take hands out.
    pub(crate) fn root_room(&self) -> usize {
        self.scenes
            .last()
            .map_or(0, |scene| scene.scene.kept_room())
    }

    // Gives back at rest the room the kept scenes took: each dropped scene
    // leaves its lists for the next on the thread, which keep room for a
    // few draws.
    pub(crate) fn trim(&mut self) {
        self.scenes.clear();
        crate::scene::trim_scene_buffer_pool();
    }

    pub(crate) fn returned_packet(&mut self) {
        if self.scenes.capacity() > self.scenes.len().saturating_mul(2) {
            self.scenes.shrink_to_fit();
        }
    }
}
