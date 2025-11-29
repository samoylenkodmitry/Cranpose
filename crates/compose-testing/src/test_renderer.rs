use compose_foundation::{PointerEvent, PointerEventKind, PointerPhase};
use compose_render_common::{HitTestTarget, RenderScene, Renderer};
use compose_ui::{LayoutBox, LayoutTree, ModifierNodeSlices, Rect};
use compose_ui_graphics::Size;
use std::rc::Rc;

#[derive(Clone)]
pub struct TestHitRegion {
    pub rect: Rect,
    pub pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    pub z_index: usize,
}

impl HitTestTarget for TestHitRegion {
    fn dispatch(&self, kind: PointerEventKind, x: f32, y: f32) {
        let local_pos = compose_ui::Point {
            x: x - self.rect.x,
            y: y - self.rect.y,
        };
        let global_pos = compose_ui::Point { x, y };

        let event = PointerEvent {
            id: 0,
            kind,
            phase: match kind {
                PointerEventKind::Down => PointerPhase::Start,
                PointerEventKind::Move => PointerPhase::Move,
                PointerEventKind::Up => PointerPhase::End,
                PointerEventKind::Cancel => PointerPhase::Cancel,
            },
            position: local_pos,
            global_position: global_pos,
            buttons: Default::default(),
        };

        for handler in &self.pointer_inputs {
            handler(event.clone());
        }
    }
}

impl TestHitRegion {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        self.rect.contains(x, y)
    }
}

#[derive(Default)]
pub struct TestScene {
    pub hits: Vec<TestHitRegion>,
    next_z: usize,
}

impl TestScene {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.hits.clear();
        self.next_z = 0;
    }

    pub fn push_hit(&mut self, rect: Rect, pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>) {
        if pointer_inputs.is_empty() {
            return;
        }
        let z_index = self.next_z;
        self.next_z += 1;
        let z_index = self.next_z;
        self.hits.push(TestHitRegion {
            rect,
            pointer_inputs,
            z_index,
        });
    }
}

impl RenderScene for TestScene {
    type HitTarget = TestHitRegion;

    fn clear(&mut self) {
        self.clear();
    }

    fn hit_test(&self, x: f32, y: f32) -> Vec<Self::HitTarget> {
        // Iterate in reverse z-order (top-most first)
        let mut hits: Vec<_> = self.hits
            .iter()
            .filter(|hit| hit.contains(x, y))
            .cloned()
            .collect();
        hits.sort_by(|a, b| b.z_index.cmp(&a.z_index));
        hits
    }
}

#[derive(Default)]
pub struct TestRenderer {
    scene: TestScene,
}

impl TestRenderer {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Renderer for TestRenderer {
    type Scene = TestScene;
    type Error = String;

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        layout_tree: &LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.clear();
        build_test_scene(layout_tree.root(), &mut self.scene);
        Ok(())
    }
}

fn build_test_scene(layout: &LayoutBox, scene: &mut TestScene) {
    let slices = layout.node_data.modifier_slices();
    if !slices.pointer_inputs().is_empty() {
         scene.push_hit(layout.rect, slices.pointer_inputs().to_vec());
    }

    for child in &layout.children {
        build_test_scene(child, scene);
    }
}
