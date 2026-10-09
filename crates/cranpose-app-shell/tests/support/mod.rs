//! A renderer for scene update tests: it builds the scene graph from the
//! applier, counts whole rebuilds and can check each scoped update against a
//! graph built from scratch.

use std::{cell::Cell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MemoryApplier, NodeId, location_key};
use cranpose_render_common::{
    Renderer, SceneUpdates,
    graph::{LayerNode, PrimitiveNode, RenderNode},
    graph_scene::Scene,
    scene_builder::{
        build_graph_from_applier, build_graph_from_layout_tree, update_graph_from_applier,
    },
};
use cranpose_ui_graphics::Size;

/// Builds the scene graph from the applier and counts whole rebuilds. With
/// `mismatches`, it builds a graph from scratch after each scoped update and
/// counts the updates whose scene differs from it, which runs every draw of
/// the screen again.
pub struct CheckingRenderer {
    pub scene: Scene,
    pub rebuilds: Rc<Cell<usize>>,
    pub mismatches: Option<Rc<Cell<usize>>>,
}

/// What a [`CheckingRenderer`] counts: whole rebuilds, and the scoped updates
/// whose scene differs from one built from scratch.
#[derive(Clone, Default)]
pub struct SceneChecks {
    pub rebuilds: Rc<Cell<usize>>,
    pub mismatches: Rc<Cell<usize>>,
}

/// A 320 by 240 shell that draws `content` through a [`CheckingRenderer`]
/// counting into the returned checks. With `check`, every scoped update is
/// compared with a scene built from scratch.
pub fn checking_shell(
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

/// What `layer` draws where, in drawing order: each layer's node, bounds and
/// placement, and each draw and text with its place.
pub fn scene_signature(layer: &LayerNode, out: &mut Vec<String>) {
    out.push(format!(
        "layer {:?} {:?} {:?}",
        layer.node_id, layer.local_bounds, layer.transform_to_parent
    ));
    for child in &layer.children {
        match child {
            RenderNode::Layer(child) => scene_signature(child, out),
            RenderNode::Primitive(primitive) => match &primitive.node {
                PrimitiveNode::Draw(draw) => out.push(format!("draw {:?}", draw.primitive)),
                PrimitiveNode::Text(text) => {
                    out.push(format!("text {} {:?}", text.text.text, text.rect));
                }
            },
            RenderNode::DrawRun(run) => {
                out.push(format!("run {:?}", run.primitives().collect::<Vec<_>>()));
            }
        }
    }
}

fn signature_of(graph: Option<&LayerNode>) -> Option<Vec<String>> {
    graph.map(|root| {
        let mut out = Vec::new();
        scene_signature(root, &mut out);
        out
    })
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

    fn rebuild_scene(
        &mut self,
        layout_tree: &cranpose_ui::LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.rebuilds.set(self.rebuilds.get() + 1);
        let graph = build_graph_from_layout_tree(layout_tree.root(), 1.0);
        self.scene.replace_graph(graph);
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.rebuilds.set(self.rebuilds.get() + 1);
        if let Some(graph) = build_graph_from_applier(applier, root, 1.0) {
            self.scene.replace_graph(graph);
        }
        Ok(())
    }

    fn update_scene_from_applier(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        viewport: Size,
        updates: SceneUpdates<'_>,
    ) -> Result<(), Self::Error> {
        let updated = self
            .scene
            .graph
            .as_mut()
            .is_some_and(|graph| update_graph_from_applier(applier, graph, updates, 1.0));
        if !updated {
            return self.rebuild_scene_from_applier(applier, root, viewport);
        }
        if let Some(mismatches) = &self.mismatches {
            let fresh = build_graph_from_applier(applier, root, 1.0);
            let retained = self.scene.graph.as_ref();
            if signature_of(fresh.as_ref().map(|graph| &graph.root))
                != signature_of(retained.map(|graph| &graph.root))
            {
                mismatches.set(mismatches.get() + 1);
            }
        }
        Ok(())
    }
}
