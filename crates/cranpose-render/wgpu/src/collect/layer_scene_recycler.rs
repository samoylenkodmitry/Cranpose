// The layer scenes of returned packets, which the next collect takes with
// their lists.

use cranpose_core::collections::growth_slack::GrowthSlack;

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

    // Gives back at rest the room the kept scenes took: each dropped scene
    // leaves its lists for the next on the thread, which give back their
    // room too.
    pub(crate) fn trim(&mut self) {
        self.scenes.clear();
        self.scenes.trim_growth_slack();
        crate::scene::trim_scene_buffer_pool();
    }
}
