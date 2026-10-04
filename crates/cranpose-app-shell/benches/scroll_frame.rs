use std::{
    alloc::System, cell::RefCell, collections::hash_map::DefaultHasher, hash::Hasher, rc::Rc,
    time::Instant,
};

use cranpose_app_shell::AppShell;
use cranpose_core::{MemoryApplier, NodeId, location_key};
use cranpose_render_common::{
    Renderer,
    graph::{LayerNode, RenderNode},
    graph_scene::Scene,
    scene_builder::{
        build_graph_from_applier, build_graph_from_layout_tree, update_graph_from_applier,
    },
};
use cranpose_ui::{
    Box, BoxSpec, Color, LayoutTree, LazyColumn, LazyColumnSpec, LazyItems, LazyListScope,
    LazyListState, Modifier, Size, composable, rememberLazyListState,
};
use stats_alloc::{INSTRUMENTED_SYSTEM, Region, StatsAlloc};

#[global_allocator]
static GLOBAL: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

const WARMUP: usize = 32;
const FRAMES: usize = 512;
const ROW_HEIGHT: f32 = 36.0;

#[derive(Default)]
struct GraphRenderer {
    scene: Scene,
    rebuilds: usize,
    updates: usize,
    dirty_ids: usize,
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

    fn rebuild_scene(&mut self, tree: &LayoutTree, _: Size) -> Result<(), ()> {
        self.rebuilds += 1;
        self.scene
            .replace_graph(build_graph_from_layout_tree(tree.root(), 1.0));
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        _: Size,
    ) -> Result<(), ()> {
        self.rebuilds += 1;
        let graph = build_graph_from_applier(applier, root, 1.0).ok_or(())?;
        self.scene.replace_graph(graph);
        Ok(())
    }

    fn update_scene_from_applier(
        &mut self,
        applier: &mut MemoryApplier,
        root: NodeId,
        viewport: Size,
        nodes: &[NodeId],
    ) -> Result<(), ()> {
        self.updates += 1;
        self.dirty_ids += nodes.len();
        let updated = self
            .scene
            .graph
            .as_mut()
            .is_some_and(|graph| update_graph_from_applier(applier, graph, nodes, 1.0));
        if !updated {
            self.rebuild_scene_from_applier(applier, root, viewport)?;
        }
        Ok(())
    }
}

#[composable]
fn ScrollContent(capture: Rc<RefCell<Option<LazyListState>>>, roots: usize) {
    let state = rememberLazyListState();
    *capture.borrow_mut() = Some(state);
    LazyColumn(
        Modifier::empty().fill_max_size(),
        state,
        LazyColumnSpec::default(),
        move |scope| {
            scope.items(
                LazyItems::new(512).key(|index| index as u64),
                move |index| {
                    for _ in 0..roots {
                        Box(
                            Modifier::empty()
                                .fill_max_width()
                                .height(ROW_HEIGHT / roots as f32)
                                .background(Color((index % 7) as f32 / 7.0, 0.4, 0.8, 1.0)),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                },
            );
        },
    );
}

fn hash_layer(layer: &LayerNode, hash: &mut DefaultHasher) {
    hash.write(
        format!(
            "{:?}{:?}{:?}{:?}{:?}{:?}",
            layer.local_bounds,
            layer.transform_to_parent,
            layer.content_offset,
            layer.translated_content_offset,
            layer.origin_in_parent,
            layer.clip_to_bounds,
        )
        .as_bytes(),
    );
    for child in &layer.children {
        match child {
            RenderNode::Layer(child) => hash_layer(child, hash),
            RenderNode::DrawRun(run) => hash.write_u64(run.recording.fingerprint()),
            RenderNode::Primitive(primitive) => hash.write(format!("{primitive:?}").as_bytes()),
        }
    }
}

fn fingerprint(shell: &AppShell<GraphRenderer>) -> u64 {
    let mut hash = DefaultHasher::new();
    hash_layer(
        &shell.scene().graph.as_ref().expect("scene graph").root,
        &mut hash,
    );
    hash.finish()
}

fn run_case(roots: usize, phase: &str, magnitude: f32) {
    let capture = Rc::new(RefCell::new(None));
    let content_capture = Rc::clone(&capture);
    let mut shell = AppShell::new(
        GraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || ScrollContent(Rc::clone(&content_capture), roots),
    );
    shell.set_buffer_size(360, 360);
    shell.set_viewport(360.0, 360.0);
    shell.update();
    let state = (*capture.borrow()).expect("lazy list state");
    let context = Rc::clone(shell.app_context());
    let initial = fingerprint(&shell);
    let frame = |tick: usize, shell: &mut AppShell<GraphRenderer>| {
        let delta = if tick.is_multiple_of(2) {
            -magnitude
        } else {
            magnitude
        };
        let consumed = context.enter(|| state.dispatch_scroll_delta(delta));
        assert!(consumed.abs() > 0.0, "each frame must move the list");
        shell.update();
    };
    frame(0, &mut shell);
    assert_ne!(initial, fingerprint(&shell), "scroll must change the graph");
    for tick in 1..WARMUP {
        frame(tick, &mut shell);
    }
    let renderer = shell.renderer();
    renderer.rebuilds = 0;
    renderer.updates = 0;
    renderer.dirty_ids = 0;
    let region = Region::new(GLOBAL);
    let started = Instant::now();
    for tick in WARMUP..WARMUP + FRAMES {
        frame(tick, &mut shell);
    }
    let elapsed = started.elapsed().as_nanos();
    let stats = region.change();
    let fingerprint = fingerprint(&shell);
    let renderer = shell.renderer();
    assert_eq!(renderer.rebuilds, 0, "scroll must use scoped scene updates");
    assert_eq!(renderer.updates, FRAMES, "every frame updates the scene");
    println!(
        "{{\"benchmark\":\"scroll_frame\",\"roots_per_item\":{roots},\"phase\":\"{phase}\",\"frames\":{FRAMES},\"allocations\":{},\"reallocations\":{},\"bytes_allocated\":{},\"ns_per_frame\":{},\"dirty_ids\":{},\"fingerprint\":{fingerprint}}}",
        stats.allocations,
        stats.reallocations,
        stats.bytes_allocated,
        elapsed / FRAMES as u128,
        renderer.dirty_ids,
    );
}

fn main() {
    for roots in [1, 9] {
        run_case(roots, "stable_retained", 0.125);
        run_case(roots, "one_row_entering", ROW_HEIGHT);
    }
}
