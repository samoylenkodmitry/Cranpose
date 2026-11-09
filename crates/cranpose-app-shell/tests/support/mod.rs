//! A renderer for scene update tests: it builds the scene graph from the
//! applier, counts whole rebuilds and can check each scoped update against a
//! graph built from scratch.

use std::{cell::Cell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MemoryApplier, NodeId, location_key};
use cranpose_render_common::{
    Renderer, SceneUpdates,
    graph_scene::Scene,
    scene_builder::build_from_applier,
    scene_store::{ItemRef, LayerRef, SceneStore},
};
use cranpose_ui_graphics::Size;

pub(crate) struct CheckingRenderer {
    pub scene: Scene,
    pub rebuilds: Rc<Cell<usize>>,
    pub mismatches: Option<Rc<Cell<usize>>>,
}

#[derive(Clone, Default)]
pub(crate) struct SceneChecks {
    pub rebuilds: Rc<Cell<usize>>,
    pub mismatches: Rc<Cell<usize>>,
}

pub(crate) fn checking_shell(
    check: bool,
    content: impl FnMut() + 'static,
) -> (AppShell<CheckingRenderer>, SceneChecks) {
    let checks = SceneChecks::default();
    let shell = AppShell::new_with_size(
        CheckingRenderer {
            scene: Scene::new(),
            rebuilds: Rc::clone(&checks.rebuilds),
            mismatches: check.then(|| Rc::clone(&checks.mismatches)),
        },
        location_key(file!(), line!(), column!()),
        content,
        (320, 240),
        (320.0, 240.0),
    );
    (shell, checks)
}

pub(crate) fn scene_signature(layer: LayerRef<'_>, out: &mut Vec<String>) {
    out.push(format!(
        "layer {:?} {:?} {:?}",
        layer.node_id(),
        layer.local_bounds(),
        layer.transform_to_parent()
    ));
    items_signature(layer, out);
}

fn items_signature(layer: LayerRef<'_>, out: &mut Vec<String>) {
    for item in layer.items() {
        match item {
            ItemRef::Layer(child) => scene_signature(child, out),
            ItemRef::Leaf(leaf) => {
                out.push(format!(
                    "inline {:?} {:?} {:?}",
                    leaf.node_id(),
                    leaf.origin(),
                    leaf.size()
                ));
                items_signature(leaf, out);
            }
            ItemRef::Draw(draw) => out.push(format!("draw {:?}", draw.primitive)),
            ItemRef::Text(text) => {
                out.push(format!("text {} {:?}", text.text.text(), text.rect));
            }
            ItemRef::Run(run) => {
                out.push(format!("run {:?}", run.primitives().collect::<Vec<_>>()));
            }
            ItemRef::Shape(shape) => {
                out.push(format!(
                    "shapes {:?}",
                    shape.primitives().collect::<Vec<_>>()
                ));
            }
        }
    }
}

pub(crate) fn signature_of(root: Option<LayerRef<'_>>) -> Option<Vec<String>> {
    root.map(|root| {
        let mut out = Vec::new();
        scene_signature(root, &mut out);
        out
    })
}

impl CheckingRenderer {
    // Builds the scene again with `rebuild` and counts the rebuild.
    fn rebuild(
        &mut self,
        rebuild: impl FnOnce(&mut Scene),
    ) -> Result<(), std::convert::Infallible> {
        self.rebuilds.set(self.rebuilds.get() + 1);
        rebuild(&mut self.scene);
        Ok(())
    }
}

impl Renderer for CheckingRenderer {
    type Scene = Scene;
    type Error = std::convert::Infallible;

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
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.rebuild(|scene| scene.rebuild_from_applier(applier, root))
    }

    fn update_scene_from_applier(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        _viewport: Size,
        updates: SceneUpdates<'_>,
    ) -> Result<(), Self::Error> {
        if updates.is_empty() {
            return Ok(());
        }
        if !self.scene.update_from_applier(applier, root, updates) {
            return self.rebuild(|_| {});
        }
        if let Some(mismatches) = &self.mismatches {
            let fresh = build_from_applier(applier, root);
            if signature_of(fresh.as_ref().and_then(SceneStore::root_layer))
                != signature_of(self.scene.root_layer())
            {
                mismatches.set(mismatches.get() + 1);
            }
        }
        Ok(())
    }
}
