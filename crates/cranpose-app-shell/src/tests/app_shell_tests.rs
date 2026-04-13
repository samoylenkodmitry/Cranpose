use super::*;
use cranpose_core::{
    __launched_effect_async_impl as launched_effect_async_impl, compositionLocalOf, location_key,
    useState, CompositionLocal, CompositionLocalProvider, MutableState,
};
use cranpose_foundation::{PointerEvent, PointerEventKind};
use cranpose_macros::composable;
use cranpose_ui::{
    BlendMode, Box, BoxSpec, Brush, Button, Color, Column, ColumnSpec, CornerRadii,
    HeadlessRenderer, IntrinsicSize, LinearArrangement, Modifier, PointerInputScope, Rect,
    RenderOp, Row, RowSpec, ScrollState, Size, Text, TextStyle, VerticalAlignment,
};
use cranpose_ui_graphics::{DrawPrimitive, GraphicsLayer, Point, RoundedCornerShape};
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

fn layout_tree_texts(tree: &cranpose_ui::LayoutTree) -> Vec<String> {
    fn collect(node: &cranpose_ui::LayoutBox, out: &mut Vec<String>) {
        if let Some(text) = node.node_data.modifier_slices().text_content() {
            out.push(text.to_string());
        }
        for child in &node.children {
            collect(child, out);
        }
    }

    let mut texts = Vec::new();
    collect(tree.root(), &mut texts);
    texts
}

fn find_layout_box_with_text<'a>(
    node: &'a cranpose_ui::LayoutBox,
    text: &str,
) -> Option<&'a cranpose_ui::LayoutBox> {
    if node.node_data.modifier_slices().text_content() == Some(text) {
        return Some(node);
    }

    node.children
        .iter()
        .find_map(|child| find_layout_box_with_text(child, text))
}

fn click_text<R>(shell: &mut AppShell<R>, text: &str)
where
    R: Renderer,
    R::Error: std::fmt::Debug,
{
    shell.update();
    let layout_tree = shell.layout_tree().expect("layout tree available");
    let node = find_layout_box_with_text(layout_tree.root(), text)
        .unwrap_or_else(|| panic!("text {text:?} not found in layout tree"));
    let center_x = node.rect.x + node.rect.width * 0.5;
    let center_y = node.rect.y + node.rect.height * 0.5;

    assert!(
        shell.set_cursor(center_x, center_y),
        "set_cursor should hover a hit target for {text:?}"
    );
    shell.update();
    assert!(shell.pointer_pressed(), "pointer down should hit {text:?}");
    shell.update();
    assert!(
        shell.pointer_released(),
        "pointer up should dispatch to {text:?}"
    );
    shell.update();
}

fn click_text_like_robot<R>(shell: &mut AppShell<R>, text: &str)
where
    R: Renderer,
    R::Error: std::fmt::Debug,
{
    shell.update();
    let layout_tree = shell.layout_tree().expect("layout tree available");
    let node = find_layout_box_with_text(layout_tree.root(), text)
        .unwrap_or_else(|| panic!("text {text:?} not found in layout tree"));
    let center_x = node.rect.x + node.rect.width * 0.5;
    let center_y = node.rect.y + node.rect.height * 0.5;

    assert!(
        shell.set_cursor(center_x, center_y),
        "set_cursor should hover a hit target for {text:?}"
    );
    assert!(shell.pointer_pressed(), "pointer down should hit {text:?}");
    shell.update();
    assert!(
        shell.pointer_released(),
        "pointer up should dispatch to {text:?}"
    );
    shell.update();
}

fn pump_like_robot<R>(shell: &mut AppShell<R>)
where
    R: Renderer,
    R::Error: std::fmt::Debug,
{
    if shell.needs_redraw() || shell.has_active_animations() {
        shell.update();
    }
}

fn live_slot_count(slots: &[(usize, String)]) -> usize {
    slots
        .iter()
        .filter(|(_, desc)| *desc != "Gap" && !desc.starts_with("Gap("))
        .count()
}

thread_local! {
    static APP_SHELL_ACTIVE_TAB_STATE: RefCell<Option<MutableState<i32>>> = const { RefCell::new(None) };
    static APP_SHELL_COUNTER_STATE: RefCell<Option<MutableState<i32>>> = const { RefCell::new(None) };
    static FRAME_STABLE_HANDLER_MODE: RefCell<Option<MutableState<bool>>> = const { RefCell::new(None) };
    static FRAME_STABLE_RENDERED_CLICKS: RefCell<Option<MutableState<i32>>> = const { RefCell::new(None) };
    static FRAME_STABLE_PENDING_CLICKS: RefCell<Option<MutableState<i32>>> = const { RefCell::new(None) };
}

fn app_shell_local_count() -> CompositionLocal<i32> {
    thread_local! {
        static LOCAL: RefCell<Option<CompositionLocal<i32>>> = const { RefCell::new(None) };
    }

    LOCAL.with(|cell| {
        let mut cell = cell.borrow_mut();
        if cell.is_none() {
            *cell = Some(compositionLocalOf(|| 0));
        }
        cell.as_ref()
            .expect("app shell local count initialized")
            .clone()
    })
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
    capture_path: Vec<cranpose_core::NodeId>,
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

    fn capture_path(&self) -> Vec<cranpose_core::NodeId> {
        self.capture_path.clone()
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

#[derive(Clone)]
struct MutableRecordingScene {
    hits: Rc<RefCell<Vec<RecordingHitTarget>>>,
}

impl MutableRecordingScene {
    fn new(hits: Rc<RefCell<Vec<RecordingHitTarget>>>) -> Self {
        Self { hits }
    }
}

impl RenderScene for MutableRecordingScene {
    type HitTarget = RecordingHitTarget;

    fn clear(&mut self) {}

    fn hit_test(&self, _x: f32, _y: f32) -> Vec<Self::HitTarget> {
        self.hits.borrow().clone()
    }

    fn find_target(&self, node_id: cranpose_core::NodeId) -> Option<Self::HitTarget> {
        self.hits
            .borrow()
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

struct MutableRecordingRenderer {
    scene: MutableRecordingScene,
}

impl MutableRecordingRenderer {
    fn new(scene: MutableRecordingScene) -> Self {
        Self { scene }
    }
}

impl Renderer for MutableRecordingRenderer {
    type Scene = MutableRecordingScene;
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

#[derive(Default)]
struct HitGraphRenderer {
    scene: cranpose_render_common::graph_scene::Scene,
}

fn collect_graph_hits(
    layer: &cranpose_render_common::graph::LayerNode,
    parent_transform: cranpose_render_common::graph::ProjectiveTransform,
    scene: &mut cranpose_render_common::graph_scene::Scene,
    parent_hit_clip: Option<Rect>,
) {
    struct SceneHitSink<'a> {
        scene: &'a mut cranpose_render_common::graph_scene::Scene,
    }

    impl cranpose_render_common::hit_graph::HitGraphSink for SceneHitSink<'_> {
        fn push_hit(
            &mut self,
            node_id: cranpose_core::NodeId,
            capture_path: &[cranpose_core::NodeId],
            geometry: cranpose_render_common::graph_scene::HitGeometry,
            shape: Option<cranpose_ui_graphics::RoundedCornerShape>,
            click_actions: &[Rc<dyn Fn(Point)>],
            pointer_inputs: &[Rc<dyn Fn(PointerEvent)>],
        ) {
            self.scene.push_hit(
                node_id,
                capture_path.to_vec(),
                geometry,
                shape,
                click_actions
                    .iter()
                    .cloned()
                    .map(cranpose_render_common::graph_scene::ClickAction::WithPoint)
                    .collect(),
                pointer_inputs.to_vec(),
            );
        }
    }

    let mut sink = SceneHitSink { scene };
    cranpose_render_common::hit_graph::collect_hits_from_graph(
        layer,
        parent_transform,
        &mut sink,
        parent_hit_clip,
    );
}

impl Renderer for HitGraphRenderer {
    type Scene = cranpose_render_common::graph_scene::Scene;
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
        self.scene.clear();
        let graph = cranpose_render_common::scene_builder::build_graph_from_layout_tree(
            layout_tree.root(),
            1.0,
        );
        collect_graph_hits(
            &graph.root,
            cranpose_render_common::graph::ProjectiveTransform::identity(),
            &mut self.scene,
            None,
        );
        self.scene.replace_graph(graph);
        Ok(())
    }

    fn rebuild_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.scene.clear();
        if let Some(graph) =
            cranpose_render_common::scene_builder::build_graph_from_applier(applier, root, 1.0)
        {
            collect_graph_hits(
                &graph.root,
                cranpose_render_common::graph::ProjectiveTransform::identity(),
                &mut self.scene,
                None,
            );
            self.scene.replace_graph(graph);
        }
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
fn frame_stable_pointer_handler_content() {
    let use_pending_handler = useState(|| false);
    let rendered_clicks = useState(|| 0i32);
    let pending_clicks = useState(|| 0i32);
    FRAME_STABLE_HANDLER_MODE.with(|slot| {
        *slot.borrow_mut() = Some(use_pending_handler);
    });
    FRAME_STABLE_RENDERED_CLICKS.with(|slot| {
        *slot.borrow_mut() = Some(rendered_clicks);
    });
    FRAME_STABLE_PENDING_CLICKS.with(|slot| {
        *slot.borrow_mut() = Some(pending_clicks);
    });

    let pending_handler = use_pending_handler.value();
    let rendered_clicks_state = rendered_clicks;
    let pending_clicks_state = pending_clicks;
    Button(
        Modifier::empty().padding(8.0),
        move || {
            if pending_handler {
                pending_clicks_state.set_value(pending_clicks_state.value() + 1);
            } else {
                rendered_clicks_state.set_value(rendered_clicks_state.value() + 1);
            }
        },
        move || {
            Text(
                if pending_handler {
                    "Pending Handler"
                } else {
                    "Rendered Handler"
                },
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}

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
    let mut baseline_live_slots = None;
    let mut peak_live_slots = 0usize;

    for frame in 0..200 {
        shell.update();
        assert!(
            shell.layout_tree().is_some(),
            "layout_tree should remain available after update cycle {frame}"
        );
        let live_slots = live_slot_count(&shell.debug_all_slots());
        baseline_live_slots.get_or_insert(live_slots);
        peak_live_slots = peak_live_slots.max(live_slots);
    }

    let baseline_live_slots = baseline_live_slots.expect("baseline live slot count");
    assert!(
        peak_live_slots <= baseline_live_slots + 64,
        "tabbed progress updates leaked live slots: baseline={baseline_live_slots} peak={peak_live_slots}",
    );
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
            capture_path: vec![1],
        },
        RecordingHitTarget {
            node_id: 2,
            consume: false,
            events: skipped_events.clone(),
            capture_path: vec![2],
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
fn captured_gesture_falls_back_to_original_targets_when_scene_targets_disappear() {
    let _guard = test_guard();
    let first_target_events = Rc::new(RefCell::new(Vec::new()));
    let rebound_target_events = Rc::new(RefCell::new(Vec::new()));
    let active_hits = Rc::new(RefCell::new(vec![RecordingHitTarget {
        node_id: 1,
        consume: false,
        events: first_target_events.clone(),
        capture_path: vec![1],
    }]));

    let root_key = location_key(file!(), line!(), column!());
    let scene = MutableRecordingScene::new(active_hits.clone());
    let mut shell = AppShell::new(
        MutableRecordingRenderer::new(scene),
        root_key,
        empty_content,
    );

    shell.set_cursor(10.0, 10.0);
    first_target_events.borrow_mut().clear();

    assert!(
        shell.pointer_pressed(),
        "down should hit the original target"
    );
    assert_eq!(
        first_target_events
            .borrow()
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![PointerEventKind::Down]
    );

    *active_hits.borrow_mut() = vec![RecordingHitTarget {
        node_id: 2,
        consume: false,
        events: rebound_target_events.clone(),
        capture_path: vec![2],
    }];

    assert!(
        shell.set_cursor(30.0, 30.0),
        "move should stay on the original captured target"
    );
    assert!(
        shell.pointer_released(),
        "up should stay on the original captured target after the scene target disappears"
    );

    assert_eq!(
        first_target_events
            .borrow()
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![
            PointerEventKind::Down,
            PointerEventKind::Move,
            PointerEventKind::Up
        ]
    );
    assert!(
        rebound_target_events.borrow().is_empty(),
        "captured gesture must not retarget to a different live node"
    );
}

#[test]
fn captured_gesture_continues_on_render_supplied_ancestor_when_child_disappears() {
    let _guard = test_guard();
    let child_events = Rc::new(RefCell::new(Vec::new()));
    let ancestor_events = Rc::new(RefCell::new(Vec::new()));
    let unrelated_events = Rc::new(RefCell::new(Vec::new()));
    let active_hits = Rc::new(RefCell::new(vec![RecordingHitTarget {
        node_id: 1,
        consume: false,
        events: child_events.clone(),
        capture_path: vec![1, 99],
    }]));

    let root_key = location_key(file!(), line!(), column!());
    let scene = MutableRecordingScene::new(active_hits.clone());
    let mut shell = AppShell::new(
        MutableRecordingRenderer::new(scene),
        root_key,
        empty_content,
    );

    shell.set_cursor(10.0, 10.0);
    child_events.borrow_mut().clear();
    ancestor_events.borrow_mut().clear();
    unrelated_events.borrow_mut().clear();
    assert!(
        shell.pointer_pressed(),
        "down should hit the original child target"
    );
    assert_eq!(
        child_events
            .borrow()
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![PointerEventKind::Down]
    );

    *active_hits.borrow_mut() = vec![
        RecordingHitTarget {
            node_id: 99,
            consume: false,
            events: ancestor_events.clone(),
            capture_path: vec![99],
        },
        RecordingHitTarget {
            node_id: 2,
            consume: false,
            events: unrelated_events.clone(),
            capture_path: vec![2],
        },
    ];

    assert!(
        shell.set_cursor(30.0, 30.0),
        "move should continue on the captured ancestor target"
    );
    assert!(
        shell.pointer_released(),
        "up should dispatch to the surviving ancestor target"
    );

    assert_eq!(
        ancestor_events
            .borrow()
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![PointerEventKind::Move, PointerEventKind::Up]
    );
    assert!(
        unrelated_events.borrow().is_empty(),
        "captured gesture must not retarget to unrelated fresh hits"
    );
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
fn pointer_dispatch_uses_rendered_frame_handlers_when_recomposition_is_pending() {
    let _guard = test_guard();
    FRAME_STABLE_HANDLER_MODE.with(|slot| slot.borrow_mut().take());
    FRAME_STABLE_RENDERED_CLICKS.with(|slot| slot.borrow_mut().take());
    FRAME_STABLE_PENDING_CLICKS.with(|slot| slot.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        frame_stable_pointer_handler_content,
    );
    shell.update();

    let layout_tree = shell.layout_tree().expect("layout tree available");
    let button = find_layout_box_with_text(layout_tree.root(), "Rendered Handler")
        .expect("rendered handler button in layout tree");
    let center_x = button.rect.x + button.rect.width * 0.5;
    let center_y = button.rect.y + button.rect.height * 0.5;

    FRAME_STABLE_HANDLER_MODE.with(|slot| {
        slot.borrow()
            .as_ref()
            .expect("handler mode state")
            .set_value(true);
    });

    assert!(
        shell.set_cursor(center_x, center_y),
        "hover should still resolve against the rendered frame"
    );
    assert!(
        shell.pointer_pressed(),
        "pointer down should hit the rendered frame target"
    );
    assert!(
        shell.pointer_released(),
        "pointer up should complete the gesture"
    );

    let rendered_clicks = FRAME_STABLE_RENDERED_CLICKS.with(|slot| {
        slot.borrow()
            .as_ref()
            .expect("rendered click counter")
            .get()
    });
    let pending_clicks = FRAME_STABLE_PENDING_CLICKS
        .with(|slot| slot.borrow().as_ref().expect("pending click counter").get());
    assert_eq!(
        rendered_clicks, 1,
        "pointer dispatch must stay on the frame the user actually saw"
    );
    assert_eq!(
        pending_clicks, 0,
        "pending recomposition handlers must not replace the rendered-frame handler before dispatch"
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
fn pointer_invalidation_without_scene_changes_rebuilds_scene() {
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
        1,
        "pure pointer invalidation should rebuild the scene so hit handlers stay fresh"
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

#[composable]
fn app_shell_scrollable_test_tab(label: &'static str) {
    let scroll_state =
        cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| state.clone());
    Column(
        Modifier::empty()
            .fill_max_size()
            .vertical_scroll(scroll_state, false),
        ColumnSpec::default(),
        move || {
            Text(label, Modifier::empty().padding(8.0), TextStyle::default());
            Text(
                "Scrollable content",
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
        },
    );
}

#[composable]
fn app_shell_scrollable_wrapper(content: impl FnMut() + 'static) {
    let scroll_state =
        cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| state.clone());
    Column(
        Modifier::empty()
            .fill_max_size()
            .vertical_scroll(scroll_state, false),
        ColumnSpec::default(),
        content,
    );
}

#[composable]
fn app_shell_counter_test_tab(counter: MutableState<i32>) {
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(
            "Counter Tab",
            Modifier::empty().padding(8.0),
            TextStyle::default(),
        );
        Text(
            format!("Counter value {}", counter.value()),
            Modifier::empty().padding(8.0),
            TextStyle::default(),
        );
    });
}

#[composable]
fn app_shell_interactive_counter_test_tab(counter: MutableState<i32>) {
    let pointer_position = useState(Point::default);
    let pointer_down = useState(|| false);
    let is_even = counter.value() % 2 == 0;

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            cranpose_core::with_key(&is_even, || {
                Text(
                    if is_even {
                        "Counter even"
                    } else {
                        "Counter odd"
                    },
                    Modifier::empty().padding(8.0),
                    TextStyle::default(),
                );
            });

            Box(
                Modifier::empty()
                    .fill_max_width()
                    .height(220.0)
                    .background(Color(0.12, 0.18, 0.28, 1.0))
                    .pointer_input((), {
                        move |scope: PointerInputScope| async move {
                            scope
                                .await_pointer_event_scope(|await_scope| async move {
                                    loop {
                                        let event = await_scope.await_pointer_event().await;
                                        match event.kind {
                                            PointerEventKind::Down => pointer_down.set(true),
                                            PointerEventKind::Up | PointerEventKind::Cancel => {
                                                pointer_down.set(false)
                                            }
                                            PointerEventKind::Move => {
                                                pointer_position.set(event.position);
                                            }
                                            PointerEventKind::Scroll
                                            | PointerEventKind::Enter
                                            | PointerEventKind::Exit => {}
                                        }
                                    }
                                })
                                .await;
                        }
                    })
                    .padding(12.0),
                BoxSpec::default(),
                move || {
                    Column(Modifier::empty(), ColumnSpec::default(), move || {
                        Text(
                            format!(
                                "Pointer {:.1},{:.1} down={}",
                                pointer_position.value().x,
                                pointer_position.value().y,
                                pointer_down.value()
                            ),
                            Modifier::empty().padding(8.0),
                            TextStyle::default(),
                        );
                        Text(
                            format!("Counter value {}", counter.value()),
                            Modifier::empty().padding(8.0),
                            TextStyle::default(),
                        );
                        Button(
                            Modifier::empty()
                                .background(Color(0.25, 0.45, 0.75, 1.0))
                                .padding(12.0),
                            move || {
                                counter.set_value(counter.value() + 1);
                            },
                            || {
                                Text(
                                    "Increment",
                                    Modifier::empty().padding(6.0),
                                    TextStyle::default(),
                                );
                            },
                        );
                    });
                },
            );
        },
    );
}

#[composable]
fn app_shell_effect_test_tab() {
    let request_counter = useState(|| 0u64);
    let status = useState(|| "Idle".to_string());
    let request_key = request_counter.get();

    launched_effect_async_impl(
        location_key(file!(), line!(), column!()),
        request_key,
        move |_scope| {
            let status = status;
            Box::pin(async move {
                if request_key == 0 {
                    return;
                }
                status.set_value(format!("Request {}", request_key));
            })
        },
    );

    let status_text = status.get();
    Column(
        Modifier::empty().fill_max_size().vertical_scroll(
            cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| state.clone()),
            false,
        ),
        ColumnSpec::default(),
        move || {
            Text(
                "Web Fetch Marker",
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
            Text(
                format!("Effect status {status_text}"),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
        },
    );
}

#[composable]
fn app_shell_composition_local_test_tab() {
    let local = app_shell_local_count();
    let counter = useState(|| 7i32);
    let provided = counter.get();
    Column(
        Modifier::empty().fill_max_size().vertical_scroll(
            cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| state.clone()),
            false,
        ),
        ColumnSpec::default(),
        move || {
            Text(
                "Composition Local Marker",
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
            CompositionLocalProvider(vec![local.provides(provided)], || {
                Text(
                    format!("READING local {}", local.current()),
                    Modifier::empty().padding(8.0),
                    TextStyle::default(),
                );
            });
        },
    );
}

#[composable]
fn app_shell_mixed_scrollable_tab_host() {
    let active_tab = useState(|| 3i32);
    let counter = useState(|| 0i32);
    APP_SHELL_ACTIVE_TAB_STATE.with(|slot| {
        *slot.borrow_mut() = Some(active_tab);
    });
    APP_SHELL_COUNTER_STATE.with(|slot| {
        *slot.borrow_mut() = Some(counter);
    });

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            Row(Modifier::empty(), RowSpec::default(), || {
                Text("Tab A", Modifier::empty(), TextStyle::default());
                Text("Tab B", Modifier::empty(), TextStyle::default());
                Text("Tab C", Modifier::empty(), TextStyle::default());
            });

            Box(
                Modifier::empty().fill_max_width().weight(1.0),
                BoxSpec::default(),
                move || {
                    let active = active_tab.value();
                    cranpose_core::with_key(&active, || match active {
                        0 => app_shell_counter_test_tab(counter),
                        1 => app_shell_composition_local_test_tab(),
                        2 => app_shell_effect_test_tab(),
                        _ => app_shell_scrollable_test_tab("Async Marker"),
                    });
                },
            );
        },
    );
}

#[composable]
fn app_shell_interactive_tab_host() {
    let active_tab = useState(|| 0i32);
    let counter = useState(|| 0i32);
    APP_SHELL_ACTIVE_TAB_STATE.with(|slot| {
        *slot.borrow_mut() = Some(active_tab);
    });
    APP_SHELL_COUNTER_STATE.with(|slot| {
        *slot.borrow_mut() = Some(counter);
    });

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            Row(Modifier::empty(), RowSpec::default(), || {
                Text("Counter", Modifier::empty(), TextStyle::default());
                Text("CompositionLocal", Modifier::empty(), TextStyle::default());
                Text("WebFetch", Modifier::empty(), TextStyle::default());
            });

            Box(
                Modifier::empty().fill_max_width().weight(1.0),
                BoxSpec::default(),
                move || {
                    let active = active_tab.value();
                    cranpose_core::with_key(&active, || {
                        app_shell_scrollable_wrapper(move || match active {
                            0 => app_shell_interactive_counter_test_tab(counter),
                            1 => app_shell_composition_local_test_tab(),
                            _ => app_shell_effect_test_tab(),
                        });
                    });
                },
            );
        },
    );
}

#[composable]
fn app_shell_interactive_clickable_tab_host() {
    let active_tab = useState(|| 0i32);
    let counter = useState(|| 0i32);

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            let counter_tab = active_tab;
            let composition_local_tab = active_tab;
            let web_fetch_tab = active_tab;
            Row(Modifier::empty(), RowSpec::default(), move || {
                Button(
                    Modifier::empty().padding(8.0),
                    move || counter_tab.set_value(0),
                    || {
                        Text("Counter App", Modifier::empty(), TextStyle::default());
                    },
                );
                Button(
                    Modifier::empty().padding(8.0),
                    move || composition_local_tab.set_value(1),
                    || {
                        Text(
                            "CompositionLocal Test",
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                    },
                );
                Button(
                    Modifier::empty().padding(8.0),
                    move || web_fetch_tab.set_value(2),
                    || {
                        Text("Web Fetch", Modifier::empty(), TextStyle::default());
                    },
                );
            });

            Box(
                Modifier::empty().fill_max_width().weight(1.0),
                BoxSpec::default(),
                move || {
                    let active = active_tab.value();
                    cranpose_core::with_key(&active, || {
                        app_shell_scrollable_wrapper(move || match active {
                            0 => app_shell_interactive_counter_test_tab(counter),
                            1 => app_shell_composition_local_test_tab(),
                            _ => app_shell_effect_test_tab(),
                        });
                    });
                },
            );
        },
    );
}

#[composable]
fn app_shell_demo_like_counter_tab() {
    let counter = useState(|| 0i32);
    let wave_state = useState(|| 0.35f32);
    let pointer_position = useState(Point::default);
    let pointer_down = useState(|| false);
    let pointer = pointer_position.get();

    APP_SHELL_COUNTER_STATE.with(|slot| {
        *slot.borrow_mut() = Some(counter);
    });

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            Box(Modifier::empty(), BoxSpec::default(), move || {
                Column(
                    Modifier::empty()
                        .padding(32.0)
                        .rounded_corners(24.0)
                        .draw_behind(move |scope| {
                            let phase = wave_state.value();
                            scope.draw_round_rect(
                                Brush::linear_gradient(vec![
                                    Color(
                                        0.12 + phase * 0.2,
                                        0.10,
                                        0.24 + (1.0 - phase) * 0.3,
                                        1.0,
                                    ),
                                    Color(
                                        0.08,
                                        0.16 + (1.0 - phase) * 0.3,
                                        0.26 + phase * 0.2,
                                        1.0,
                                    ),
                                ]),
                                cranpose_ui::CornerRadii::uniform(24.0),
                            );
                        })
                        .padding(20.0),
                    ColumnSpec::default(),
                    move || {
                        Text(
                            format!("Counter: {}", counter.get()),
                            Modifier::empty()
                                .padding(8.0)
                                .background(Color(0.0, 0.0, 0.0, 0.35))
                                .rounded_corners(12.0),
                            TextStyle::default(),
                        );

                        Column(
                            Modifier::empty()
                                .rounded_corners(20.0)
                                .draw_with_cache(|cache| {
                                    cache.on_draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(Color(0.16, 0.18, 0.26, 0.95)),
                                            cranpose_ui::CornerRadii::uniform(20.0),
                                        );
                                    });
                                })
                                .draw_with_content({
                                    let position = pointer_position.get();
                                    let pressed = pointer_down.get();
                                    move |scope| {
                                        let intensity = if pressed { 0.45 } else { 0.25 };
                                        scope.draw_round_rect(
                                            Brush::radial_gradient(
                                                vec![
                                                    Color(0.4, 0.6, 1.0, intensity),
                                                    Color(0.2, 0.3, 0.6, 0.0),
                                                ],
                                                position,
                                                120.0,
                                            ),
                                            cranpose_ui::CornerRadii::uniform(20.0),
                                        );
                                    }
                                })
                                .pointer_input((), {
                                    move |scope: PointerInputScope| async move {
                                        scope
                                            .await_pointer_event_scope(|await_scope| async move {
                                                loop {
                                                    let event =
                                                        await_scope.await_pointer_event().await;
                                                    match event.kind {
                                                        PointerEventKind::Down => {
                                                            pointer_down.set(true)
                                                        }
                                                        PointerEventKind::Up
                                                        | PointerEventKind::Cancel => {
                                                            pointer_down.set(false)
                                                        }
                                                        PointerEventKind::Move => {
                                                            pointer_position.set(event.position);
                                                        }
                                                        PointerEventKind::Scroll
                                                        | PointerEventKind::Enter
                                                        | PointerEventKind::Exit => {}
                                                    }
                                                }
                                            })
                                            .await;
                                    }
                                })
                                .padding(16.0),
                            ColumnSpec::default(),
                            move || {
                                Text(
                                    format!("Pointer: ({:.1}, {:.1})", pointer.x, pointer.y),
                                    Modifier::empty()
                                        .padding(8.0)
                                        .background(Color(0.1, 0.1, 0.15, 0.6))
                                        .rounded_corners(12.0)
                                        .padding(8.0),
                                    TextStyle::default(),
                                );

                                Row(
                                    Modifier::empty()
                                        .padding(8.0)
                                        .rounded_corners(12.0)
                                        .background(Color(0.1, 0.1, 0.15, 0.6))
                                        .padding(8.0),
                                    RowSpec::default(),
                                    move || {
                                        Button(
                                            Modifier::empty()
                                                .rounded_corners(16.0)
                                                .draw_with_cache(|cache| {
                                                    cache.on_draw_behind(|scope| {
                                                        scope.draw_round_rect(
                                                            Brush::linear_gradient(vec![
                                                                Color(0.2, 0.45, 0.9, 1.0),
                                                                Color(0.15, 0.3, 0.65, 1.0),
                                                            ]),
                                                            cranpose_ui::CornerRadii::uniform(16.0),
                                                        );
                                                    });
                                                })
                                                .padding(12.0),
                                            move || counter.set(counter.get() + 1),
                                            || {
                                                Text(
                                                    "Increment",
                                                    Modifier::empty().padding(6.0),
                                                    TextStyle::default(),
                                                );
                                            },
                                        );
                                    },
                                );
                            },
                        );
                    },
                );
            });
        },
    );
}

#[composable]
fn app_shell_demo_like_clickable_tab_host() {
    let active_tab = useState(|| 0i32);

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            let counter_tab = active_tab;
            let composition_local_tab = active_tab;
            Row(Modifier::empty(), RowSpec::default(), move || {
                Button(
                    Modifier::empty().padding(8.0),
                    move || counter_tab.set_value(0),
                    || {
                        Text("Counter App", Modifier::empty(), TextStyle::default());
                    },
                );
                Button(
                    Modifier::empty().padding(8.0),
                    move || composition_local_tab.set_value(1),
                    || {
                        Text(
                            "CompositionLocal Test",
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                    },
                );
            });

            Box(
                Modifier::empty().fill_max_width().weight(1.0),
                BoxSpec::default(),
                move || {
                    let active = active_tab.value();
                    cranpose_core::with_key(&active, || {
                        app_shell_scrollable_wrapper(move || match active {
                            0 => app_shell_demo_like_counter_tab(),
                            _ => app_shell_composition_local_test_tab(),
                        });
                    });
                },
            );
        },
    );
}

#[composable]
fn app_shell_actual_like_counter_tab() {
    cranpose_core::debug_label_current_scope("actual_like_counter_tab");
    let counter = useState(|| 0i32);
    let wave_state = useState(|| 0.35f32);
    let pointer_position = useState(Point::default);
    let pointer_down = useState(|| false);
    let async_message = useState(|| "Tap \"Fetch async value\" to run background work".to_string());
    let fetch_request = useState(|| 0u64);
    let pointer = pointer_position.get();
    let is_even = counter.get() % 2 == 0;

    APP_SHELL_COUNTER_STATE.with(|slot| {
        *slot.borrow_mut() = Some(counter);
    });

    Column(
        Modifier::empty().fill_max_size().padding(24.0),
        ColumnSpec::default(),
        move || {
            cranpose_core::with_key(&is_even, move || {
                if is_even {
                    Text(
                        "if counter % 2 == 0",
                        Modifier::empty().padding(8.0),
                        TextStyle::default(),
                    );
                } else {
                    Text(
                        "if counter % 2 != 0",
                        Modifier::empty().padding(8.0),
                        TextStyle::default(),
                    );
                }
            });

            Text(
                "Cranpose Playground",
                Modifier::empty()
                    .padding(12.0)
                    .then(
                        Modifier::empty()
                            .rounded_corner_shape(RoundedCornerShape::new(16.0, 24.0, 16.0, 24.0)),
                    )
                    .draw_with_content(|scope| {
                        scope.draw_round_rect(
                            Brush::solid(Color(1.0, 1.0, 1.0, 0.1)),
                            CornerRadii::uniform(20.0),
                        );
                    }),
                TextStyle::default(),
            );

            Row(
                Modifier::empty().fill_max_width().padding(8.0),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    Text(
                        format!("Counter: {}", counter.get()),
                        Modifier::empty()
                            .padding(8.0)
                            .then(Modifier::empty().background(Color(0.0, 0.0, 0.0, 0.35)))
                            .rounded_corners(12.0),
                        TextStyle::default(),
                    );
                    Text(
                        "Wave layer-only animation",
                        Modifier::empty()
                            .padding(8.0)
                            .then(Modifier::empty().background(Color(0.35, 0.55, 0.9, 0.5)))
                            .rounded_corners(12.0)
                            .graphics_layer(move || {
                                let wave_value = wave_state.value();
                                GraphicsLayer {
                                    alpha: 0.7 + wave_value * 0.3,
                                    scale: 0.85 + wave_value * 0.3,
                                    translation_y: (wave_value - 0.5) * 12.0,
                                    ..Default::default()
                                }
                            }),
                        TextStyle::default(),
                    );
                },
            );

            Column(
                Modifier::empty()
                    .rounded_corners(20.0)
                    .draw_with_cache(|cache| {
                        cache.on_draw_behind(|scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color(0.16, 0.18, 0.26, 0.95)),
                                CornerRadii::uniform(20.0),
                            );
                        });
                    })
                    .draw_with_content({
                        let position = pointer_position.get();
                        let pressed = pointer_down.get();
                        move |scope| {
                            let intensity = if pressed { 0.45 } else { 0.25 };
                            scope.draw_round_rect(
                                Brush::radial_gradient(
                                    vec![
                                        Color(0.4, 0.6, 1.0, intensity),
                                        Color(0.2, 0.3, 0.6, 0.0),
                                    ],
                                    position,
                                    120.0,
                                ),
                                CornerRadii::uniform(20.0),
                            );
                        }
                    })
                    .pointer_input((), {
                        move |scope: PointerInputScope| async move {
                            scope
                                .await_pointer_event_scope(|await_scope| async move {
                                    loop {
                                        let event = await_scope.await_pointer_event().await;
                                        match event.kind {
                                            PointerEventKind::Down => pointer_down.set(true),
                                            PointerEventKind::Up | PointerEventKind::Cancel => {
                                                pointer_down.set(false)
                                            }
                                            PointerEventKind::Move => {
                                                pointer_position.set(Point {
                                                    x: event.position.x,
                                                    y: event.position.y,
                                                });
                                            }
                                            PointerEventKind::Scroll
                                            | PointerEventKind::Enter
                                            | PointerEventKind::Exit => {}
                                        }
                                    }
                                })
                                .await;
                        }
                    })
                    .padding(16.0),
                ColumnSpec::default(),
                move || {
                    Text(
                        format!("Pointer: ({:.1}, {:.1})", pointer.x, pointer.y),
                        Modifier::empty()
                            .padding(8.0)
                            .background(Color(0.1, 0.1, 0.15, 0.6))
                            .rounded_corners(12.0)
                            .padding(8.0),
                        TextStyle::default(),
                    );

                    Row(
                        Modifier::empty()
                            .padding(8.0)
                            .rounded_corners(12.0)
                            .background(Color(0.1, 0.1, 0.15, 0.6))
                            .padding(8.0),
                        RowSpec::new()
                            .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
                            .vertical_alignment(VerticalAlignment::CenterVertically),
                        || {
                            for (label, color) in [
                                ("OK", Color(0.3, 0.5, 0.2, 1.0)),
                                ("Cancel", Color(0.5, 0.3, 0.2, 1.0)),
                                ("Long Button Text", Color(0.2, 0.3, 0.5, 1.0)),
                            ] {
                                Button(
                                    Modifier::empty()
                                        .width_intrinsic(IntrinsicSize::Max)
                                        .rounded_corners(12.0)
                                        .draw_behind(move |scope| {
                                            scope.draw_round_rect(
                                                Brush::solid(color),
                                                CornerRadii::uniform(12.0),
                                            );
                                        })
                                        .padding(10.0),
                                    || {},
                                    move || {
                                        Text(
                                            label,
                                            Modifier::empty().padding(4.0),
                                            TextStyle::default(),
                                        );
                                    },
                                );
                            }
                        },
                    );

                    Row(
                        Modifier::empty().padding(8.0),
                        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(12.0)),
                        move || {
                            Button(
                                Modifier::empty()
                                    .rounded_corners(16.0)
                                    .draw_with_cache(|cache| {
                                        cache.on_draw_behind(|scope| {
                                            scope.draw_round_rect(
                                                Brush::linear_gradient(vec![
                                                    Color(0.2, 0.45, 0.9, 1.0),
                                                    Color(0.15, 0.3, 0.65, 1.0),
                                                ]),
                                                CornerRadii::uniform(16.0),
                                            );
                                        });
                                    })
                                    .padding(12.0),
                                move || counter.set(counter.get() + 1),
                                || {
                                    Text(
                                        "Increment",
                                        Modifier::empty().padding(6.0),
                                        TextStyle::default(),
                                    );
                                },
                            );
                            Button(
                                Modifier::empty()
                                    .rounded_corners(16.0)
                                    .draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(Color(0.4, 0.18, 0.3, 1.0)),
                                            CornerRadii::uniform(16.0),
                                        );
                                    })
                                    .padding(12.0),
                                move || counter.set(counter.get() - 1),
                                || {
                                    Text(
                                        "Decrement",
                                        Modifier::empty().padding(6.0),
                                        TextStyle::default(),
                                    );
                                },
                            );
                        },
                    );

                    Text(
                        async_message.get(),
                        Modifier::empty()
                            .padding(10.0)
                            .background(Color(0.1, 0.18, 0.32, 0.6))
                            .rounded_corners(14.0),
                        TextStyle::default(),
                    );

                    Button(
                        Modifier::empty()
                            .rounded_corners(16.0)
                            .draw_with_cache(|cache| {
                                cache.on_draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::linear_gradient(vec![
                                            Color(0.15, 0.35, 0.85, 1.0),
                                            Color(0.08, 0.2, 0.55, 1.0),
                                        ]),
                                        CornerRadii::uniform(16.0),
                                    );
                                });
                            })
                            .padding(12.0),
                        {
                            move || {
                                async_message
                                    .set(format!("Background fetch #{}", fetch_request.get() + 1));
                                fetch_request.update(|value| *value += 1);
                            }
                        },
                        || {
                            Text(
                                "Fetch async value",
                                Modifier::empty().padding(6.0),
                                TextStyle::default(),
                            );
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn app_shell_actual_like_clickable_tab_host() {
    let active_tab = useState(|| 0i32);

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            let counter_tab = active_tab;
            let composition_local_tab = active_tab;
            Row(Modifier::empty(), RowSpec::default(), move || {
                Button(
                    Modifier::empty().padding(8.0),
                    move || counter_tab.set_value(0),
                    || {
                        Text("Counter App", Modifier::empty(), TextStyle::default());
                    },
                );
                Button(
                    Modifier::empty().padding(8.0),
                    move || composition_local_tab.set_value(1),
                    || {
                        Text(
                            "CompositionLocal Test",
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                    },
                );
            });

            Box(
                Modifier::empty()
                    .fill_max_width()
                    .weight(1.0)
                    .graphics_layer(|| GraphicsLayer {
                        blend_mode: BlendMode::SrcOver,
                        ..Default::default()
                    }),
                BoxSpec::default(),
                move || {
                    let active = active_tab.value();
                    cranpose_core::with_key(&active, || {
                        app_shell_scrollable_wrapper(move || match active {
                            0 => app_shell_actual_like_counter_tab(),
                            _ => app_shell_composition_local_test_tab(),
                        });
                    });
                },
            );
        },
    );
}

#[composable]
fn app_shell_many_tabs_clickable_host() {
    cranpose_core::debug_label_current_scope("many_tabs_host");
    const TAB_LABELS: [&str; 18] = [
        "Counter App",
        "CompositionLocal Test",
        "Async Runtime",
        "Animations",
        "Web Fetch",
        "Text Input",
        "Layout",
        "Modifiers Showcase",
        "Lazy List",
        "Mineswapper",
        "Hacker News",
        "Images",
        "Text",
        "Winamp",
        "Xkcd",
        "Shaders",
        "Shader Rect",
        "Markdown Viewer",
    ];

    let active_tab = useState(|| 0i32);

    #[composable]
    fn tab_button(index: i32, label: &'static str, active_tab: MutableState<i32>) {
        let _ = label;
        cranpose_core::debug_label_current_scope("many_tabs_tab_button");
        let is_active = active_tab.get() == index;
        Button(
            Modifier::empty()
                .rounded_corners(12.0)
                .draw_behind(move |scope| {
                    scope.draw_round_rect(
                        Brush::solid(if is_active {
                            Color(0.2, 0.45, 0.9, 1.0)
                        } else {
                            Color(0.3, 0.3, 0.3, 0.5)
                        }),
                        CornerRadii::uniform(12.0),
                    );
                })
                .padding(10.0),
            move || {
                if active_tab.get() != index {
                    active_tab.set_value(index);
                }
            },
            move || {
                Text(label, Modifier::empty().padding(4.0), TextStyle::default());
            },
        );
    }

    Column(
        Modifier::empty().fill_max_size().padding(20.0),
        ColumnSpec::default(),
        move || {
            let tabs_scroll_state =
                cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| state.clone());
            Row(
                Modifier::empty()
                    .fill_max_width()
                    .padding(8.0)
                    .clip_to_bounds()
                    .horizontal_scroll(tabs_scroll_state, false),
                RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    for (index, label) in TAB_LABELS.iter().enumerate() {
                        tab_button(index as i32, label, active_tab);
                    }
                },
            );

            Box(
                Modifier::empty().fill_max_width().weight(1.0),
                BoxSpec::default(),
                move || {
                    let active = active_tab.get();
                    cranpose_core::with_key(&active, || {
                        cranpose_core::debug_label_current_scope("many_tabs_tab_content");
                        app_shell_scrollable_wrapper(move || match active {
                            0 => app_shell_actual_like_counter_tab(),
                            1 => app_shell_composition_local_test_tab(),
                            _ => {
                                Text(
                                    format!("Tab {}", active),
                                    Modifier::empty().padding(8.0),
                                    TextStyle::default(),
                                );
                            }
                        });
                    });
                },
            );
        },
    );
}

fn app_shell_many_tab_requires_scroll(tab: i32) -> bool {
    !matches!(tab, 10 | 8 | 13 | 17)
}

#[composable]
fn app_shell_many_tabs_precise_tab_button(
    index: i32,
    label: &'static str,
    active_tab: MutableState<i32>,
) {
    let is_active = active_tab.get() == index;
    Button(
        Modifier::empty()
            .rounded_corners(12.0)
            .draw_behind(move |scope| {
                scope.draw_round_rect(
                    Brush::solid(if is_active {
                        Color(0.2, 0.45, 0.9, 1.0)
                    } else {
                        Color(0.3, 0.3, 0.3, 0.5)
                    }),
                    CornerRadii::uniform(12.0),
                );
            })
            .padding(10.0),
        move || {
            if active_tab.get() != index {
                active_tab.set_value(index);
            }
        },
        move || {
            Text(label, Modifier::empty().padding(4.0), TextStyle::default());
        },
    );
}

#[composable]
fn app_shell_many_tabs_precise_tab_bar(active_tab: MutableState<i32>) {
    const TAB_LABELS: [&str; 18] = [
        "Counter App",
        "CompositionLocal Test",
        "Async Runtime",
        "Animations",
        "Web Fetch",
        "Text Input",
        "Layout",
        "Modifiers Showcase",
        "Lazy List",
        "Mineswapper",
        "Hacker News",
        "Images",
        "Text",
        "Winamp",
        "Xkcd",
        "Shaders",
        "Shader Rect",
        "Markdown Viewer",
    ];

    let tabs_scroll_state =
        cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| state.clone());
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding(8.0)
            .clip_to_bounds()
            .horizontal_scroll(tabs_scroll_state, false),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            for (index, label) in TAB_LABELS.iter().enumerate() {
                app_shell_many_tabs_precise_tab_button(index as i32, label, active_tab);
            }
        },
    );
}

#[composable]
fn app_shell_many_tabs_precise_render_active(active: i32) {
    match active {
        0 => app_shell_actual_like_counter_tab(),
        1 => app_shell_composition_local_test_tab(),
        _ => {
            Text(
                format!("Tab {}", active),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
        }
    }
}

#[composable]
fn app_shell_many_tabs_precise_tab_content(active_tab: MutableState<i32>, modifier: Modifier) {
    let active = active_tab.get();
    Box(modifier.clip_to_bounds(), BoxSpec::default(), move || {
        cranpose_core::with_key(&active, || {
            if app_shell_many_tab_requires_scroll(active) {
                app_shell_scrollable_wrapper(move || {
                    app_shell_many_tabs_precise_render_active(active)
                });
            } else {
                app_shell_many_tabs_precise_render_active(active);
            }
        });
    });
}

#[composable]
fn app_shell_many_tabs_precise_clickable_host() {
    let active_tab = useState(|| 0i32);

    Column(
        Modifier::empty().fill_max_size().padding(20.0),
        ColumnSpec::default(),
        move || {
            app_shell_many_tabs_precise_tab_bar(active_tab);
            Box(
                Modifier::empty().fill_max_width().weight(1.0),
                BoxSpec::default(),
                move || {
                    app_shell_many_tabs_precise_tab_content(
                        active_tab,
                        Modifier::empty().fill_max_width().weight(1.0),
                    );
                },
            );
        },
    );
}

#[composable]
fn app_shell_animated_draw_counter_tab() {
    let phase_state = useState(|| 0.35f32);

    Column(
        Modifier::empty()
            .fill_max_size()
            .padding(16.0)
            .draw_behind(move |scope| {
                let phase = phase_state.value();
                scope.draw_round_rect(
                    Brush::solid(Color(0.15 + phase * 0.2, 0.2, 0.35, 1.0)),
                    cranpose_ui::CornerRadii::uniform(12.0),
                );
            }),
        ColumnSpec::default(),
        move || {
            Text(
                "Animated Draw Counter",
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );
            Button(
                Modifier::empty().padding(8.0),
                move || phase_state.set_value(0.8),
                || {
                    Text("Drive wave", Modifier::empty(), TextStyle::default());
                },
            );
        },
    );
}

fn app_shell_animated_draw_tab_host() {
    let active_tab = useState(|| 0i32);
    APP_SHELL_ACTIVE_TAB_STATE.with(|slot| {
        *slot.borrow_mut() = Some(active_tab);
    });

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            Box(
                Modifier::empty().fill_max_width().weight(1.0),
                BoxSpec::default(),
                move || {
                    let active = active_tab.value();
                    cranpose_core::with_key(&active, || {
                        app_shell_scrollable_wrapper(move || match active {
                            0 => app_shell_animated_draw_counter_tab(),
                            _ => app_shell_composition_local_test_tab(),
                        });
                    });
                },
            );
        },
    );
}

#[test]
fn semantics_enabled_shell_keeps_scrollable_tab_content_after_mixed_switches() {
    let _guard = test_guard();
    APP_SHELL_ACTIVE_TAB_STATE.with(|slot| slot.borrow_mut().take());
    APP_SHELL_COUNTER_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        TestRenderer::default(),
        root_key,
        app_shell_mixed_scrollable_tab_host,
    );

    shell.set_semantics_enabled(true);
    shell.update();

    let active_tab = APP_SHELL_ACTIVE_TAB_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("active tab state registered");

    active_tab.set_value(0);
    shell.update();
    let counter_texts = layout_tree_texts(shell.layout_tree().expect("counter layout tree"));
    assert!(
        counter_texts
            .iter()
            .any(|text| text.contains("Counter value 0")),
        "counter tab content missing after first switch: {counter_texts:?}",
    );

    active_tab.set_value(1);
    shell.update();
    let first_scrollable =
        layout_tree_texts(shell.layout_tree().expect("first scrollable layout tree"));
    assert!(
        first_scrollable
            .iter()
            .any(|text| text.contains("Composition Local Marker")),
        "first scrollable tab content missing: {first_scrollable:?}",
    );
    assert!(
        first_scrollable
            .iter()
            .any(|text| text.contains("READING local 7")),
        "composition local content missing after switch: {first_scrollable:?}",
    );

    active_tab.set_value(2);
    shell.update();
    let second_scrollable =
        layout_tree_texts(shell.layout_tree().expect("second scrollable layout tree"));
    assert!(
        second_scrollable
            .iter()
            .any(|text| text.contains("Web Fetch Marker")),
        "second scrollable tab content missing after mixed switches: {second_scrollable:?}",
    );
    assert!(
        second_scrollable
            .iter()
            .any(|text| text.contains("Effect status Idle")),
        "effect tab content missing after mixed switches: {second_scrollable:?}",
    );

    active_tab.set_value(1);
    shell.update();
    let restored_local = layout_tree_texts(
        shell
            .layout_tree()
            .expect("restored composition local layout tree"),
    );
    assert!(
        restored_local
            .iter()
            .any(|text| text.contains("Composition Local Marker")),
        "composition local tab content missing after revisit: {restored_local:?}",
    );

    active_tab.set_value(2);
    shell.update();
    let restored_effect =
        layout_tree_texts(shell.layout_tree().expect("restored effect layout tree"));
    assert!(
        restored_effect
            .iter()
            .any(|text| text.contains("Web Fetch Marker")),
        "effect tab content missing after revisit: {restored_effect:?}",
    );

    let counter_state = APP_SHELL_COUNTER_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("counter state registered");
    active_tab.set_value(0);
    shell.update();
    counter_state.set_value(1);
    shell.update();
    let updated_counter =
        layout_tree_texts(shell.layout_tree().expect("updated counter layout tree"));
    assert!(
        updated_counter
            .iter()
            .any(|text| text.contains("Counter value 1")),
        "counter tab did not update after tab walk: {updated_counter:?}",
    );
}

#[test]
fn headless_shell_counter_click_updates_after_restored_scrollable_tab_walk() {
    let _guard = test_guard();
    APP_SHELL_ACTIVE_TAB_STATE.with(|slot| slot.borrow_mut().take());
    APP_SHELL_COUNTER_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_interactive_tab_host,
    );

    shell.set_semantics_enabled(true);
    shell.update();

    let active_tab = APP_SHELL_ACTIVE_TAB_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("active tab state registered");
    let counter_state = APP_SHELL_COUNTER_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("counter state registered");

    assert_eq!(counter_state.value(), 0, "counter should start at zero");

    active_tab.set_value(1);
    shell.update();
    let texts = layout_tree_texts(shell.layout_tree().expect("composition local layout"));
    assert!(
        texts
            .iter()
            .any(|text| text.contains("Composition Local Marker")),
        "composition local content missing after first switch: {texts:?}",
    );

    active_tab.set_value(2);
    shell.update();
    let texts = layout_tree_texts(shell.layout_tree().expect("web fetch layout"));
    assert!(
        texts.iter().any(|text| text.contains("Web Fetch Marker")),
        "web fetch content missing after first visit: {texts:?}",
    );

    active_tab.set_value(1);
    shell.update();
    let texts = layout_tree_texts(shell.layout_tree().expect("restored local layout"));
    assert!(
        texts.iter().any(|text| text.contains("READING local 7")),
        "composition local content missing after revisit: {texts:?}",
    );

    active_tab.set_value(0);
    shell.update();
    let texts = layout_tree_texts(shell.layout_tree().expect("counter layout"));
    assert!(
        texts.iter().any(|text| text.contains("Counter value 0")),
        "counter tab content missing after restore: {texts:?}",
    );

    let interactive_box = find_layout_box_with_text(
        shell.layout_tree().expect("counter layout tree").root(),
        "Pointer 0.0,0.0 down=false",
    )
    .expect("pointer text in layout tree");
    let start_x = interactive_box.rect.x + interactive_box.rect.width * 0.5;
    let start_y = interactive_box.rect.y + 80.0;

    for step in 0..12 {
        let x = start_x + step as f32 * 4.0;
        let y = start_y + step as f32 * 6.0;
        let _ = shell.set_cursor(x, y);
        shell.update();
    }

    click_text(&mut shell, "Increment");

    let texts = layout_tree_texts(shell.layout_tree().expect("updated counter layout"));
    assert_eq!(
        counter_state.value(),
        1,
        "button click should mutate the counter state; texts={texts:?}",
    );
    assert!(
        texts.iter().any(|text| text.contains("Counter value 1")),
        "counter text did not update after restored tab walk click: {texts:?}",
    );
}

#[test]
fn headless_shell_counter_click_updates_after_tab_button_roundtrip() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_interactive_clickable_tab_host,
    );

    shell.set_semantics_enabled(true);
    shell.update();

    click_text(&mut shell, "CompositionLocal Test");
    let texts = layout_tree_texts(shell.layout_tree().expect("composition local layout"));
    assert!(
        texts
            .iter()
            .any(|text| text.contains("Composition Local Marker")),
        "composition local content missing after tab click: {texts:?}",
    );

    click_text(&mut shell, "Counter App");
    let texts = layout_tree_texts(shell.layout_tree().expect("counter layout"));
    assert!(
        texts.iter().any(|text| text.contains("Counter value 0")),
        "counter content missing after return click: {texts:?}",
    );

    let interactive_box = find_layout_box_with_text(
        shell.layout_tree().expect("counter layout tree").root(),
        "Pointer 0.0,0.0 down=false",
    )
    .expect("pointer text in layout tree");
    let start_x = interactive_box.rect.x + interactive_box.rect.width * 0.5;
    let start_y = interactive_box.rect.y + 80.0;

    for step in 0..12 {
        let x = start_x + step as f32 * 4.0;
        let y = start_y + step as f32 * 6.0;
        let _ = shell.set_cursor(x, y);
        shell.update();
    }

    click_text(&mut shell, "Increment");

    let texts = layout_tree_texts(shell.layout_tree().expect("updated counter layout"));
    assert!(
        texts.iter().any(|text| text.contains("Counter value 1")),
        "counter text did not update after tab button roundtrip click: {texts:?}",
    );
}

#[test]
fn headless_shell_render_graph_survives_restored_draw_state() {
    let _guard = test_guard();
    APP_SHELL_ACTIVE_TAB_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_animated_draw_tab_host,
    );

    shell.update();

    let active_tab = APP_SHELL_ACTIVE_TAB_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("active tab state registered");

    active_tab.set_value(1);
    shell.update();

    active_tab.set_value(0);
    shell.update();

    let texts = layout_tree_texts(shell.layout_tree().expect("animated draw layout"));
    assert!(
        texts
            .iter()
            .any(|text| text.contains("Animated Draw Counter")),
        "animated draw counter content missing after restore: {texts:?}",
    );
}

#[test]
fn headless_shell_demo_like_counter_click_updates_after_tab_roundtrip() {
    let _guard = test_guard();
    APP_SHELL_COUNTER_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_demo_like_clickable_tab_host,
    );

    shell.set_semantics_enabled(true);
    shell.update();

    click_text(&mut shell, "CompositionLocal Test");
    let texts = layout_tree_texts(shell.layout_tree().expect("composition local layout"));
    assert!(
        texts
            .iter()
            .any(|text| text.contains("Composition Local Marker")),
        "composition local content missing after tab click: {texts:?}",
    );

    click_text(&mut shell, "Counter App");
    let texts = layout_tree_texts(shell.layout_tree().expect("counter layout"));
    assert!(
        texts.iter().any(|text| text.contains("Counter: 0")),
        "counter content missing after return click: {texts:?}",
    );

    let pointer_text = find_layout_box_with_text(
        shell.layout_tree().expect("counter layout tree").root(),
        "Pointer: (0.0, 0.0)",
    )
    .expect("pointer text in layout tree");
    let start_x = pointer_text.rect.x + pointer_text.rect.width * 0.5;
    let start_y = pointer_text.rect.y + 80.0;

    for step in 0..12 {
        let x = start_x + step as f32 * 4.0;
        let y = start_y + step as f32 * 6.0;
        let _ = shell.set_cursor(x, y);
        shell.update();
    }

    click_text(&mut shell, "Increment");

    let texts = layout_tree_texts(shell.layout_tree().expect("updated counter layout"));
    assert!(
        texts.iter().any(|text| text.contains("Counter: 1")),
        "demo-like counter text did not update after restored tab walk click: {texts:?}",
    );
}

#[test]
fn headless_shell_demo_like_counter_click_updates_with_robot_style_pumps() {
    let _guard = test_guard();
    APP_SHELL_COUNTER_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_demo_like_clickable_tab_host,
    );

    shell.set_semantics_enabled(true);
    shell.update();

    click_text(&mut shell, "CompositionLocal Test");
    click_text(&mut shell, "Counter App");

    let pointer_text = find_layout_box_with_text(
        shell.layout_tree().expect("counter layout tree").root(),
        "Pointer: (0.0, 0.0)",
    )
    .expect("pointer text in layout tree");
    let start_x = pointer_text.rect.x + pointer_text.rect.width * 0.5;
    let start_y = pointer_text.rect.y + 80.0;

    for step in 0..12 {
        let x = start_x + step as f32 * 4.0;
        let y = start_y + step as f32 * 6.0;
        let _ = shell.set_cursor(x, y);
        pump_like_robot(&mut shell);
    }

    let increment = find_layout_box_with_text(
        shell.layout_tree().expect("counter layout tree").root(),
        "Increment",
    )
    .expect("increment button in layout tree");
    let button_x = increment.rect.x + increment.rect.width * 0.5;
    let button_y = increment.rect.y + increment.rect.height * 0.5;

    let _ = shell.set_cursor(button_x, button_y);
    pump_like_robot(&mut shell);
    assert!(shell.pointer_pressed(), "pointer down should hit increment");
    pump_like_robot(&mut shell);
    assert!(shell.pointer_released(), "pointer up should hit increment");
    pump_like_robot(&mut shell);

    let texts = layout_tree_texts(shell.layout_tree().expect("updated counter layout"));
    assert!(
        texts.iter().any(|text| text.contains("Counter: 1")),
        "demo-like counter text did not update with robot-style pumping: {texts:?}",
    );
}

#[test]
fn headless_shell_actual_like_counter_click_updates_with_robot_click_order() {
    let _guard = test_guard();
    APP_SHELL_COUNTER_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_actual_like_clickable_tab_host,
    );

    shell.set_semantics_enabled(true);
    shell.update();

    click_text_like_robot(&mut shell, "CompositionLocal Test");
    click_text_like_robot(&mut shell, "Counter App");

    let pointer_text = find_layout_box_with_text(
        shell.layout_tree().expect("counter layout tree").root(),
        "Pointer: (0.0, 0.0)",
    )
    .expect("pointer text in layout tree");
    let start_x = pointer_text.rect.x + pointer_text.rect.width * 0.5;
    let start_y = pointer_text.rect.y + 80.0;

    for step in 0..20 {
        let progress = step as f32 / 19.0;
        let x = start_x + (80.0 - start_x) * progress;
        let y = start_y + (230.0 - start_y) * progress;
        let _ = shell.set_cursor(x, y);
        pump_like_robot(&mut shell);
    }

    let increment = find_layout_box_with_text(
        shell.layout_tree().expect("counter layout tree").root(),
        "Increment",
    )
    .expect("increment button in layout tree");
    let button_x = increment.rect.x + increment.rect.width * 0.5;
    let button_y = increment.rect.y + increment.rect.height * 0.5;

    let _ = shell.set_cursor(button_x, button_y);
    pump_like_robot(&mut shell);
    assert!(shell.set_cursor(button_x, button_y));
    assert!(shell.pointer_pressed(), "pointer down should hit increment");
    pump_like_robot(&mut shell);
    assert!(shell.pointer_released(), "pointer up should hit increment");
    pump_like_robot(&mut shell);

    let texts = layout_tree_texts(shell.layout_tree().expect("updated counter layout"));
    assert!(
        texts.iter().any(|text| text.contains("Counter: 1")),
        "actual-like counter text did not update after robot-style click order: {texts:?}",
    );
}

#[test]
fn headless_shell_many_tabs_counter_click_updates_with_robot_click_order() {
    let _guard = test_guard();
    APP_SHELL_COUNTER_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_many_tabs_clickable_host,
    );

    shell.set_semantics_enabled(true);
    shell.update();

    click_text_like_robot(&mut shell, "CompositionLocal Test");
    click_text_like_robot(&mut shell, "Counter App");

    let pointer_text = find_layout_box_with_text(
        shell.layout_tree().expect("counter layout tree").root(),
        "Pointer: (0.0, 0.0)",
    )
    .expect("pointer text in layout tree");
    let start_x = pointer_text.rect.x + pointer_text.rect.width * 0.5;
    let start_y = pointer_text.rect.y + 80.0;

    for step in 0..20 {
        let progress = step as f32 / 19.0;
        let x = start_x + (80.0 - start_x) * progress;
        let y = start_y + (230.0 - start_y) * progress;
        let _ = shell.set_cursor(x, y);
        pump_like_robot(&mut shell);
    }

    let increment = find_layout_box_with_text(
        shell.layout_tree().expect("counter layout tree").root(),
        "Increment",
    )
    .expect("increment button in layout tree");
    let button_x = increment.rect.x + increment.rect.width * 0.5;
    let button_y = increment.rect.y + increment.rect.height * 0.5;

    let _ = shell.set_cursor(button_x, button_y);
    pump_like_robot(&mut shell);
    assert!(shell.set_cursor(button_x, button_y));
    assert!(shell.pointer_pressed(), "pointer down should hit increment");
    pump_like_robot(&mut shell);
    assert!(shell.pointer_released(), "pointer up should hit increment");
    pump_like_robot(&mut shell);

    let texts = layout_tree_texts(shell.layout_tree().expect("updated counter layout"));
    assert!(
        texts.iter().any(|text| text.contains("Counter: 1")),
        "many-tab counter text did not update after robot-style click order: {texts:?}",
    );
}

#[test]
fn headless_shell_many_tabs_precise_counter_click_updates_with_robot_click_order() {
    let _guard = test_guard();
    APP_SHELL_COUNTER_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_many_tabs_precise_clickable_host,
    );

    shell.set_semantics_enabled(true);
    shell.update();

    click_text_like_robot(&mut shell, "CompositionLocal Test");
    click_text_like_robot(&mut shell, "Counter App");

    let pointer_text = find_layout_box_with_text(
        shell.layout_tree().expect("counter layout tree").root(),
        "Pointer: (0.0, 0.0)",
    )
    .expect("pointer text in layout tree");
    let start_x = pointer_text.rect.x + pointer_text.rect.width * 0.5;
    let start_y = pointer_text.rect.y + 80.0;

    for step in 0..20 {
        let progress = step as f32 / 19.0;
        let x = start_x + (80.0 - start_x) * progress;
        let y = start_y + (230.0 - start_y) * progress;
        let _ = shell.set_cursor(x, y);
        pump_like_robot(&mut shell);
    }

    let increment = find_layout_box_with_text(
        shell.layout_tree().expect("counter layout tree").root(),
        "Increment",
    )
    .expect("increment button in layout tree");
    let button_x = increment.rect.x + increment.rect.width * 0.5;
    let button_y = increment.rect.y + increment.rect.height * 0.5;

    let _ = shell.set_cursor(button_x, button_y);
    pump_like_robot(&mut shell);
    assert!(shell.set_cursor(button_x, button_y));
    assert!(shell.pointer_pressed(), "pointer down should hit increment");
    pump_like_robot(&mut shell);
    assert!(shell.pointer_released(), "pointer up should hit increment");
    pump_like_robot(&mut shell);

    let texts = layout_tree_texts(shell.layout_tree().expect("updated counter layout"));
    assert!(
        texts.iter().any(|text| text.contains("Counter: 1")),
        "many-tab precise counter text did not update after robot-style click order: {texts:?}",
    );
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
