use super::*;
use cranpose_core::{
    __launched_effect_async_impl as launched_effect_async_impl, location_key, useState,
};
use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_macros::composable;
use cranpose_ui::{
    Box, BoxSpec, Brush, Color, Column, ColumnSpec, HeadlessRenderer, Modifier, Rect, RenderOp,
    Row, RowSpec, Size, Text, TextStyle,
};
use cranpose_ui_graphics::{DrawPrimitive, Point};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Mutex, MutexGuard, OnceLock};

fn test_guard() -> MutexGuard<'static, ()> {
    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("test lock poisoned")
}

#[derive(Default, Clone)]
struct TestHitTarget;

impl HitTestTarget for TestHitTarget {
    fn dispatch(&self, _event: PointerEvent) {}

    fn node_id(&self) -> cranpose_core::NodeId {
        0
    }
}

#[derive(Default)]
struct TestScene;

impl RenderScene for TestScene {
    type HitTarget = TestHitTarget;

    fn clear(&mut self) {}

    fn hit_test(&self, _x: f32, _y: f32) -> Vec<Self::HitTarget> {
        vec![]
    }

    fn find_target(&self, _node_id: cranpose_core::NodeId) -> Option<Self::HitTarget> {
        None
    }
}

#[derive(Clone)]
struct RecordingHitTarget {
    node_id: cranpose_core::NodeId,
    consume: bool,
    events: Rc<RefCell<Vec<PointerEvent>>>,
}

impl HitTestTarget for RecordingHitTarget {
    fn dispatch(&self, event: PointerEvent) {
        self.events.borrow_mut().push(event.clone());
        if self.consume {
            event.consume();
        }
    }

    fn node_id(&self) -> cranpose_core::NodeId {
        self.node_id
    }
}

#[derive(Default)]
struct RecordingScene {
    hits: Vec<RecordingHitTarget>,
}

impl RecordingScene {
    fn with_hits(hits: Vec<RecordingHitTarget>) -> Self {
        Self { hits }
    }
}

impl RenderScene for RecordingScene {
    type HitTarget = RecordingHitTarget;

    fn clear(&mut self) {}

    fn hit_test(&self, _x: f32, _y: f32) -> Vec<Self::HitTarget> {
        self.hits.clone()
    }

    fn find_target(&self, node_id: cranpose_core::NodeId) -> Option<Self::HitTarget> {
        self.hits
            .iter()
            .find(|target| target.node_id == node_id)
            .cloned()
    }
}

#[derive(Default)]
struct TestRenderer {
    scene: TestScene,
}

impl Renderer for TestRenderer {
    type Scene = TestScene;
    type Error = ();

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        _layout_tree: &LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        _applier: &mut cranpose_core::MemoryApplier,
        _root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

struct ScrollDispatchRenderer {
    scene: RecordingScene,
}

impl ScrollDispatchRenderer {
    fn new(scene: RecordingScene) -> Self {
        Self { scene }
    }
}

impl Renderer for ScrollDispatchRenderer {
    type Scene = RecordingScene;
    type Error = ();

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        _layout_tree: &LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        _applier: &mut cranpose_core::MemoryApplier,
        _root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Default)]
struct RecordingRenderer {
    scene: TestScene,
    last_scene: Option<cranpose_ui::RecordedRenderScene>,
}

impl Renderer for RecordingRenderer {
    type Scene = TestScene;
    type Error = ();

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
        let renderer = HeadlessRenderer::new();
        self.last_scene = Some(renderer.render(layout_tree));
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        let renderer = HeadlessRenderer::new();
        self.last_scene = Some(renderer.render_from_applier(applier, root));
        Ok(())
    }
}

struct CountingRenderer {
    scene: TestScene,
    rebuilds: Rc<Cell<usize>>,
}

impl CountingRenderer {
    fn new(rebuilds: Rc<Cell<usize>>) -> Self {
        Self {
            scene: TestScene,
            rebuilds,
        }
    }
}

impl Renderer for CountingRenderer {
    type Scene = TestScene;
    type Error = ();

    fn scene(&self) -> &Self::Scene {
        &self.scene
    }

    fn scene_mut(&mut self) -> &mut Self::Scene {
        &mut self.scene
    }

    fn rebuild_scene(
        &mut self,
        _layout_tree: &LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.rebuilds.set(self.rebuilds.get() + 1);
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        _applier: &mut cranpose_core::MemoryApplier,
        _root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.rebuilds.set(self.rebuilds.get() + 1);
        Ok(())
    }
}

#[composable]
fn tabbed_progress_content() {
    let progress = useState(|| 0.6f32);
    let active_tab = useState(|| 0i32);

    let progress_effect = progress;
    let active_effect = active_tab;
    launched_effect_async_impl(
        location_key(file!(), line!(), column!()),
        (),
        move |scope| {
            let progress = progress_effect;
            let active_tab = active_effect;
            Box::pin(async move {
                let clock = scope.runtime().frame_clock();
                let mut phase: u32 = 0;
                while scope.is_active() {
                    let _ = clock.next_frame().await;
                    if !scope.is_active() {
                        break;
                    }
                    match phase % 3 {
                        0 => {
                            progress.set_value(0.0);
                            active_tab.set_value(1);
                        }
                        1 => {
                            progress.set_value(0.85);
                        }
                        _ => {
                            active_tab.set_value(0);
                        }
                    }
                    phase = phase.wrapping_add(1);
                }
            })
        },
    );

    Column(
        Modifier::empty().padding(8.0),
        ColumnSpec::default(),
        move || {
            Text(
                format!("Progress {:.2}", progress.value()),
                Modifier::empty().padding(2.0),
                TextStyle::default(),
            );
            let progress_for_branch = progress;
            let active_for_branch = active_tab;
            Row(
                Modifier::empty()
                    .padding(2.0)
                    .then(Modifier::empty().height(12.0)),
                RowSpec::default(),
                move || {
                    if active_for_branch.value() == 0 && progress_for_branch.value() > 0.0 {
                        let progress_for_bar = progress_for_branch;
                        Row(
                            Modifier::empty()
                                .width(160.0 * progress_for_bar.value())
                                .then(Modifier::empty().height(12.0)),
                            RowSpec::default(),
                            move || {
                                let _ = progress_for_bar.value();
                            },
                        );
                    }
                },
            );
        },
    );
}

#[composable]
fn empty_content() {}

#[composable]
fn box_content() {
    Box(
        Modifier::empty().size(Size {
            width: 24.0,
            height: 24.0,
        }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn semantics_content() {
    Text(
        "Semantics",
        Modifier::empty().semantics(|config| {
            config.content_description = Some("Semantics".into());
        }),
        TextStyle::default(),
    );
}

#[composable]
fn nested_branch_content() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Box(
            Modifier::empty().size(Size {
                width: 40.0,
                height: 20.0,
            }),
            BoxSpec::default(),
            || {
                Box(
                    Modifier::empty().size(Size {
                        width: 10.0,
                        height: 10.0,
                    }),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
        Box(
            Modifier::empty().size(Size {
                width: 50.0,
                height: 20.0,
            }),
            BoxSpec::default(),
            || {
                Box(
                    Modifier::empty().size(Size {
                        width: 11.0,
                        height: 11.0,
                    }),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    });
}

#[composable]
fn draw_width_app(width_state: cranpose_core::MutableState<f32>) {
    Box(
        Modifier::empty()
            .size(Size {
                width: 200.0,
                height: 40.0,
            })
            .draw_behind({
                let width = width_state.get();
                move |scope| {
                    scope.draw_rect_at(
                        Rect {
                            x: 0.0,
                            y: 0.0,
                            width,
                            height: 10.0,
                        },
                        Brush::solid(Color(0.9, 0.1, 0.1, 1.0)),
                    );
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

struct DeleteSurroundingHandler {
    last_delete: Cell<Option<(usize, usize)>>,
}

impl cranpose_ui::text_field_focus::FocusedTextFieldHandler for DeleteSurroundingHandler {
    fn handle_key(&self, _event: &cranpose_ui::KeyEvent) -> bool {
        false
    }

    fn insert_text(&self, _text: &str) {}

    fn delete_surrounding(&self, before_bytes: usize, after_bytes: usize) {
        self.last_delete.set(Some((before_bytes, after_bytes)));
    }

    fn copy_selection(&self) -> Option<String> {
        None
    }

    fn cut_selection(&self) -> Option<String> {
        None
    }

    fn set_composition(&self, _text: &str, _cursor: Option<(usize, usize)>) {}
}

#[test]
fn layout_recovers_after_tab_switching_updates() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, || {
        tabbed_progress_content()
    });

    for frame in 0..200 {
        shell.update();
        assert!(
            shell.layout_tree().is_some(),
            "layout_tree should remain available after update cycle {frame}"
        );
    }
}

#[test]
fn ime_delete_surrounding_marks_dirty() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, empty_content);
    shell.update();
    assert!(!shell.needs_redraw());

    let focus_flag = Rc::new(RefCell::new(false));
    let handler = Rc::new(DeleteSurroundingHandler {
        last_delete: Cell::new(None),
    });

    cranpose_ui::text_field_focus::request_focus(focus_flag, handler.clone());
    assert!(shell.on_ime_delete_surrounding(2, 1));
    assert_eq!(handler.last_delete.get(), Some((2, 1)));
    assert!(shell.needs_redraw());
    cranpose_ui::text_field_focus::clear_focus();
}

#[test]
fn pointer_scrolled_dispatches_to_hovered_targets_and_respects_consumption() {
    let _guard = test_guard();
    let consumed_events = Rc::new(RefCell::new(Vec::new()));
    let skipped_events = Rc::new(RefCell::new(Vec::new()));
    let scene = RecordingScene::with_hits(vec![
        RecordingHitTarget {
            node_id: 1,
            consume: true,
            events: consumed_events.clone(),
        },
        RecordingHitTarget {
            node_id: 2,
            consume: false,
            events: skipped_events.clone(),
        },
    ]);

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(ScrollDispatchRenderer::new(scene), root_key, empty_content);
    shell.set_cursor(20.0, 30.0);
    consumed_events.borrow_mut().clear();
    skipped_events.borrow_mut().clear();

    let consumed = shell.pointer_scrolled(12.0, -18.0);
    assert!(consumed, "wheel dispatch should report consumption");
    assert_eq!(consumed_events.borrow().len(), 1);
    assert_eq!(skipped_events.borrow().len(), 0);

    let events = consumed_events.borrow();
    let event = events.first().expect("expected scroll event");
    assert_eq!(event.kind, PointerEventKind::Scroll);
    assert_eq!(event.scroll_delta, Point { x: 12.0, y: -18.0 });
    assert_eq!(event.global_position, Point { x: 20.0, y: 30.0 });
}

#[test]
fn pointer_scrolled_returns_false_without_hit_targets() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, empty_content);
    shell.set_cursor(5.0, 7.0);

    assert!(
        !shell.pointer_scrolled(0.0, 32.0),
        "wheel dispatch should return false when no handlers are hit"
    );
}

#[test]
fn draw_repass_updates_render_data_without_layout() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let state_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
        Rc::new(RefCell::new(None));
    let state_holder_for_app = Rc::clone(&state_holder);

    let mut shell = AppShell::new(RecordingRenderer::default(), root_key, move || {
        let width_state = useState(|| 24.0f32);
        *state_holder_for_app.borrow_mut() = Some(width_state);
        draw_width_app(width_state);
    });

    shell.update();
    let initial_scene = shell
        .renderer
        .last_scene
        .as_ref()
        .expect("expected initial render scene");
    let initial_width = find_rect_width(initial_scene, Color(0.9, 0.1, 0.1, 1.0))
        .expect("expected initial draw rect");

    let width_state = state_holder
        .borrow()
        .as_ref()
        .cloned()
        .expect("width state should be captured");
    width_state.set(120.0);

    shell
        .composition
        .process_invalid_scopes()
        .expect("recompose after width change");
    shell.run_render_phase();

    let updated_scene = shell
        .renderer
        .last_scene
        .as_ref()
        .expect("expected updated render scene");
    let updated_width = find_rect_width(updated_scene, Color(0.9, 0.1, 0.1, 1.0))
        .expect("expected updated draw rect");

    assert_ne!(initial_width, updated_width, "draw width should update");
    assert!(
        (updated_width - 120.0).abs() < 0.1,
        "updated width should reflect latest state"
    );
}

#[test]
fn render_invalidation_without_scene_changes_rebuilds_scene() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let rebuilds = Rc::new(Cell::new(0));
    let mut shell = AppShell::new(
        CountingRenderer::new(Rc::clone(&rebuilds)),
        root_key,
        box_content,
    );

    shell.update();
    rebuilds.set(0);

    cranpose_ui::request_render_invalidation();
    shell.run_render_phase();

    assert_eq!(
        rebuilds.get(),
        1,
        "pure render invalidation should rebuild scene for render-only updates"
    );
}

#[test]
fn pointer_invalidation_without_scene_changes_skips_rebuild() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let rebuilds = Rc::new(Cell::new(0));
    let mut shell = AppShell::new(
        CountingRenderer::new(Rc::clone(&rebuilds)),
        root_key,
        box_content,
    );

    shell.update();
    rebuilds.set(0);

    let root = shell.composition.root().expect("expected composition root");
    shell
        .composition
        .applier_mut()
        .with_node::<LayoutNode, _>(root, |node| {
            node.mark_needs_pointer_pass();
        })
        .expect("expected layout root node");
    cranpose_ui::schedule_pointer_repass(root);
    cranpose_ui::request_pointer_invalidation();

    shell.process_frame();

    assert_eq!(
        rebuilds.get(),
        0,
        "pure pointer invalidation should reuse the retained scene"
    );
    let needs_pointer_pass = shell
        .composition
        .applier_mut()
        .with_node::<LayoutNode, _>(root, |node| node.needs_pointer_pass())
        .expect("expected layout root node");
    assert!(
        !needs_pointer_pass,
        "pointer dispatch queue should clear the node dirty flag"
    );
}

#[test]
fn focus_invalidation_without_scene_changes_skips_rebuild() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let rebuilds = Rc::new(Cell::new(0));
    let mut shell = AppShell::new(
        CountingRenderer::new(Rc::clone(&rebuilds)),
        root_key,
        box_content,
    );

    shell.update();
    rebuilds.set(0);

    let root = shell.composition.root().expect("expected composition root");
    shell
        .composition
        .applier_mut()
        .with_node::<LayoutNode, _>(root, |node| {
            node.mark_needs_focus_sync();
        })
        .expect("expected layout root node");
    cranpose_ui::schedule_focus_invalidation(root);
    cranpose_ui::request_focus_invalidation();

    shell.process_frame();

    assert_eq!(
        rebuilds.get(),
        0,
        "pure focus invalidation should reuse the retained scene"
    );
    let needs_focus_sync = shell
        .composition
        .applier_mut()
        .with_node::<LayoutNode, _>(root, |node| node.needs_focus_sync())
        .expect("expected layout root node");
    assert!(
        !needs_focus_sync,
        "focus dispatch queue should clear the node dirty flag"
    );
}

#[test]
fn semantics_collection_is_opt_in_for_app_shell() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, semantics_content);

    assert!(
        shell.semantics_tree().is_none(),
        "app shell should skip semantics work until a consumer is enabled"
    );

    shell.set_semantics_enabled(true);
    shell.process_frame();
    assert!(
        shell.semantics_tree().is_some(),
        "enabling semantics should rebuild the tree on the next frame"
    );

    shell.set_semantics_enabled(false);
    assert!(
        shell.semantics_tree().is_none(),
        "disabling semantics should drop the cached tree"
    );
}

#[test]
fn draw_refresh_scope_only_contains_dirty_ancestors() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, nested_branch_content);

    shell.update();
    let layout_tree = shell.layout_tree().expect("expected layout tree");
    let root = node_id_at_path(layout_tree.root(), &[]);
    let left = node_id_at_path(layout_tree.root(), &[0]);
    let left_leaf = node_id_at_path(layout_tree.root(), &[0, 0]);
    let right = node_id_at_path(layout_tree.root(), &[1]);
    let right_leaf = node_id_at_path(layout_tree.root(), &[1, 0]);

    let dirty_nodes = HashSet::from([left_leaf]);
    let refresh_scope = {
        let mut applier = shell.composition.applier_mut();
        build_draw_refresh_scope(&mut applier, &dirty_nodes)
    };

    assert!(refresh_scope.contains(&root));
    assert!(refresh_scope.contains(&left));
    assert!(refresh_scope.contains(&left_leaf));
    assert!(!refresh_scope.contains(&right));
    assert!(!refresh_scope.contains(&right_leaf));
}

#[test]
fn layout_bounds_index_matches_cached_layout_tree() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, nested_branch_content);

    shell.update();

    let (root_id, root_bounds, left_leaf_id, left_leaf_bounds, right_id, right_bounds) = {
        let layout_tree = shell.layout_tree().expect("expected layout tree");
        let root = layout_box_at_path(layout_tree.root(), &[]);
        let left_leaf = layout_box_at_path(layout_tree.root(), &[0, 0]);
        let right = layout_box_at_path(layout_tree.root(), &[1]);

        (
            root.node_id,
            (root.rect.x, root.rect.y, root.rect.width, root.rect.height),
            left_leaf.node_id,
            (
                left_leaf.rect.x,
                left_leaf.rect.y,
                left_leaf.rect.width,
                left_leaf.rect.height,
            ),
            right.node_id,
            (
                right.rect.x,
                right.rect.y,
                right.rect.width,
                right.rect.height,
            ),
        )
    };

    assert_eq!(
        shell.root_layout_size(),
        Some((root_bounds.2, root_bounds.3))
    );
    assert_eq!(shell.node_layout_bounds(root_id), Some(root_bounds));
    assert_eq!(
        shell.node_layout_bounds(left_leaf_id),
        Some(left_leaf_bounds)
    );
    assert_eq!(shell.node_layout_bounds(right_id), Some(right_bounds));
}

fn layout_box_at_path<'a>(
    layout: &'a cranpose_ui::LayoutBox,
    path: &[usize],
) -> &'a cranpose_ui::LayoutBox {
    let mut current = layout;
    for &index in path {
        current = current
            .children
            .get(index)
            .expect("expected layout child at path");
    }
    current
}

fn node_id_at_path(layout: &cranpose_ui::LayoutBox, path: &[usize]) -> cranpose_core::NodeId {
    layout_box_at_path(layout, path).node_id
}

fn find_rect_width(scene: &cranpose_ui::RecordedRenderScene, color: Color) -> Option<f32> {
    for op in scene.operations() {
        if let RenderOp::Primitive {
            primitive: DrawPrimitive::Rect { rect, brush },
            ..
        } = op
        {
            if *brush == Brush::solid(color) {
                return Some(rect.width);
            }
        }
    }
    None
}
