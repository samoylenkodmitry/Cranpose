use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{Mutex, MutexGuard, OnceLock},
    time::Duration,
};

use cranpose_core::{
    __launched_effect_async_impl as launched_effect_async_impl, CompositionLocal,
    CompositionLocalProvider, MutableState, TaskSite, compositionLocalOf, location_key,
    rememberMutableStateOf,
};
use cranpose_foundation::{
    Modifiers, PointerEvent, PointerEventKind, PointerSource,
    lazy::{LazyListScope, LazyListState, rememberLazyListState},
};
use cranpose_macros::composable;
use cranpose_ui::{
    Alignment, BlendMode, Box, BoxSpec, Brush, Button, ButtonSpec, Color, Column, ColumnSpec,
    CornerRadii, HeadlessRenderer, IntrinsicSize, LazyColumn, LazyColumnSpec, LinearArrangement,
    Modifier, PointerInputScope, Rect, RenderOp, Row, RowSpec, ScrollState, Size, Spacer,
    SubcomposeLayoutScope, SubcomposeMeasureScope, Text, TextStyle, VerticalAlignment,
};
use cranpose_ui_graphics::{
    CompositingStrategy, DrawPrimitive, GraphicsLayer, Point, RenderEffect, RoundedCornerShape,
    RuntimeShader,
};

use super::*;

fn test_guard() -> MutexGuard<'static, ()> {
    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    match TEST_LOCK.get_or_init(|| Mutex::new(())).lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn reset_public_render_state_for_test() {}

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

fn semantics_tree_descriptions(tree: &cranpose_ui::SemanticsTree) -> Vec<String> {
    fn collect(node: &cranpose_ui::SemanticsNode, out: &mut Vec<String>) {
        if let Some(description) = &node.description {
            out.push(description.clone());
        }
        for child in &node.children {
            collect(child, out);
        }
    }

    let mut descriptions = Vec::new();
    collect(tree.root(), &mut descriptions);
    descriptions
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

fn live_slot_count(slots: &[cranpose_core::SlotDebugEntry]) -> usize {
    slots.len()
}

thread_local! {
    static APP_SHELL_LAZY_LIST_STATE: RefCell<Option<LazyListState>> = const { RefCell::new(None) };
}

thread_local! {
    static APP_SHELL_FRAME_TIME_RECORDS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

thread_local! {
    static APP_SHELL_CONTINUOUS_FRAME_COUNT: Cell<u32> = const { Cell::new(0) };
}

thread_local! {
    static APP_SHELL_INITIAL_DENSITIES: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
}

thread_local! {
    static APP_SHELL_WHEEL_SCROLL_STATE: RefCell<Option<ScrollState>> = const { RefCell::new(None) };
}

#[composable]
#[allow(non_snake_case)]
fn AppShellCaptureInitialDensity() {
    APP_SHELL_INITIAL_DENSITIES.with(|densities| {
        densities.borrow_mut().push(cranpose_ui::current_density());
    });
}

#[composable]
#[allow(non_snake_case)]
fn AppShellScrollIndicatorLazyList() {
    let list_state = rememberLazyListState();
    APP_SHELL_LAZY_LIST_STATE.with(|slot| {
        *slot.borrow_mut() = Some(list_state);
    });

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            Text(
                format!("First visible {}", list_state.first_visible_item_index()),
                Modifier::empty(),
                TextStyle::default(),
            );
            LazyColumn(
                Modifier::empty().fill_max_width().weight(1.0),
                list_state,
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                |scope| {
                    scope.items(80, |index| {
                        Text(
                            format!("Row {}", index),
                            Modifier::empty().height(48.0),
                            TextStyle::default(),
                        );
                    });
                },
            );
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn AppShellChildFirstVisible(list_state: LazyListState) {
    Text(
        format!(
            "Child first visible {}",
            list_state.first_visible_item_index()
        ),
        Modifier::empty(),
        TextStyle::default(),
    );
}

#[composable]
#[allow(non_snake_case)]
fn AppShellChildStats(list_state: LazyListState) {
    let stats = list_state.stats();
    Text(
        format!("Child visible {}", stats.items_in_use),
        Modifier::empty(),
        TextStyle::default(),
    );
}

#[composable]
#[allow(non_snake_case)]
fn AppShellSiblingIndicatorsLazyList() {
    let list_state = rememberLazyListState();
    APP_SHELL_LAZY_LIST_STATE.with(|slot| {
        *slot.borrow_mut() = Some(list_state);
    });

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            AppShellChildStats(list_state);
            AppShellChildFirstVisible(list_state);
            LazyColumn(
                Modifier::empty().fill_max_width().weight(1.0),
                list_state,
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                |scope| {
                    scope.items(80, |index| {
                        Text(
                            format!("Row {}", index),
                            Modifier::empty().height(48.0),
                            TextStyle::default(),
                        );
                    });
                },
            );
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn AppShellVariableHeightSiblingIndicatorsLazyList() {
    let list_state = rememberLazyListState();
    APP_SHELL_LAZY_LIST_STATE.with(|slot| {
        *slot.borrow_mut() = Some(list_state);
    });

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            AppShellChildStats(list_state);
            AppShellChildFirstVisible(list_state);
            LazyColumn(
                Modifier::empty().fill_max_width().weight(1.0),
                list_state,
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                |scope| {
                    scope.items(100, |index| {
                        Text(
                            format!("Row {}", index),
                            Modifier::empty().height(48.0 + (index % 5) as f32 * 8.0),
                            TextStyle::default(),
                        );
                    });
                },
            );
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn AppShellLifecycleCountDisplay(count: MutableState<usize>) {
    Text(
        format!("Lifecycle count {}", count.get()),
        Modifier::empty(),
        TextStyle::default(),
    );
}

#[composable]
#[allow(non_snake_case)]
fn AppShellLifecycleListItem(index: usize, count: MutableState<usize>) {
    cranpose_core::DisposableEffect!(index, move |_| {
        count.update(|current| *current += 1);
        cranpose_core::DisposableEffectResult::new(|| {})
    });

    Text(
        format!("Row {}", index),
        Modifier::empty().height(48.0 + (index % 5) as f32 * 8.0),
        TextStyle::default(),
    );
}

#[composable]
#[allow(non_snake_case)]
fn AppShellLifecycleIndicatorsLazyList() {
    let list_state = rememberLazyListState();
    APP_SHELL_LAZY_LIST_STATE.with(|slot| {
        *slot.borrow_mut() = Some(list_state);
    });
    let lifecycle_count = cranpose_core::rememberMutableStateOf(|| 0usize);

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            AppShellLifecycleCountDisplay(lifecycle_count);
            AppShellChildStats(list_state);
            AppShellChildFirstVisible(list_state);
            let lifecycle_count_for_items = lifecycle_count;
            LazyColumn(
                Modifier::empty().fill_max_width().weight(1.0),
                list_state,
                LazyColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                move |scope| {
                    let lifecycle_count = lifecycle_count_for_items;
                    scope.items(100, move |index| {
                        AppShellLifecycleListItem(index, lifecycle_count);
                    });
                },
            );
        },
    );
}

thread_local! {
    static APP_SHELL_ACTIVE_TAB_STATE: RefCell<Option<MutableState<i32>>> = const { RefCell::new(None) };
    static APP_SHELL_COUNTER_STATE: RefCell<Option<MutableState<i32>>> = const { RefCell::new(None) };
    static FRAME_STABLE_HANDLER_MODE: RefCell<Option<MutableState<bool>>> = const { RefCell::new(None) };
    static FRAME_STABLE_RENDERED_CLICKS: RefCell<Option<MutableState<i32>>> = const { RefCell::new(None) };
    static FRAME_STABLE_PENDING_CLICKS: RefCell<Option<MutableState<i32>>> = const { RefCell::new(None) };
    static ROOT_RENDER_TEST_INVALIDATED: Cell<bool> = const { Cell::new(false) };
}

#[composable]
#[allow(non_snake_case)]
fn AppShellKeyedSiblingIndicatorsRoot() {
    let active = cranpose_core::rememberMutableStateOf(|| 0i32);
    APP_SHELL_ACTIVE_TAB_STATE.with(|slot| {
        *slot.borrow_mut() = Some(active);
    });

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            Text(
                format!("Tab {}", active.get()),
                Modifier::empty(),
                TextStyle::default(),
            );
            cranpose_core::with_key(&active.get(), || {
                AppShellSiblingIndicatorsLazyList();
            });
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn AppShellSwitchingKeyedLazyListRoot() {
    let active = cranpose_core::rememberMutableStateOf(|| 0i32);
    APP_SHELL_ACTIVE_TAB_STATE.with(|slot| {
        *slot.borrow_mut() = Some(active);
    });

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            cranpose_core::with_key(&active.get(), || {
                if active.get() == 0 {
                    Text("Counter branch", Modifier::empty(), TextStyle::default());
                } else {
                    AppShellSiblingIndicatorsLazyList();
                }
            });
        },
    );
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

#[composable]
fn callbackless_root_render_probe(render_count: Rc<Cell<usize>>) {
    let root_trigger = rememberMutableStateOf(|| false);
    render_count.set(render_count.get() + 1);
    cranpose_core::with_key(&"root-render-probe", || {
        let _ = root_trigger.value();
    });

    Text(
        format!("Render {}", render_count.get()),
        Modifier::empty(),
        TextStyle::default(),
    );

    cranpose_core::SideEffect(move || {
        let already_invalidated = ROOT_RENDER_TEST_INVALIDATED.with(|flag| flag.replace(true));
        if already_invalidated {
            return;
        }
        root_trigger.set_value(true);
    });
}

#[composable]
#[allow(non_snake_case)]
fn AppShellAnimatedLazyItem() {
    let list_state = rememberLazyListState();
    LazyColumn(
        Modifier::empty().fill_max_width().height(120.0),
        list_state,
        LazyColumnSpec::default(),
        |scope| {
            scope.item_keyed(Some(0), None, || {
                let pulse = rememberMutableStateOf(|| 0u32);
                let run_token = rememberMutableStateOf(|| 0u64);
                let current_run_token = run_token.value();
                let pulse_for_effect = pulse;
                launched_effect_async_impl(
                    location_key(file!(), line!(), column!()),
                    TaskSite::new(file!(), line!()),
                    current_run_token,
                    move |scope| {
                        let pulse = pulse_for_effect;
                        Box::pin(async move {
                            if current_run_token == 0 {
                                return;
                            }
                            let clock = scope.runtime().frame_clock();
                            while scope.is_active() {
                                let _ = clock.next_frame().await;
                                if !scope.is_active() {
                                    break;
                                }
                                pulse.set_value(pulse.get_non_reactive().wrapping_add(1));
                            }
                        })
                    },
                );
                cranpose_core::SideEffect(move || {
                    if run_token.value() == 0 {
                        run_token.set_value(1);
                    }
                });
                let pulse_value = pulse.value();
                Text(
                    format!("Lazy Pulse: {pulse_value}"),
                    Modifier::empty().height(24.0),
                    TextStyle::default(),
                );
            });
        },
    );
}

fn first_semantics_description_with_prefix(
    shell: &mut AppShell<TestRenderer>,
    prefix: &str,
) -> Option<String> {
    semantics_tree_descriptions(shell.semantics_tree()?)
        .into_iter()
        .find(|description| description.starts_with(prefix))
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
    hit_node_ids: Option<Vec<cranpose_core::NodeId>>,
}

impl RecordingScene {
    fn with_hits(hits: Vec<RecordingHitTarget>) -> Self {
        Self {
            hits,
            hit_node_ids: None,
        }
    }

    fn with_hit_node_ids(
        hits: Vec<RecordingHitTarget>,
        hit_node_ids: Vec<cranpose_core::NodeId>,
    ) -> Self {
        Self {
            hits,
            hit_node_ids: Some(hit_node_ids),
        }
    }
}

impl RenderScene for RecordingScene {
    type HitTarget = RecordingHitTarget;

    fn clear(&mut self) {}

    fn hit_test(&self, _x: f32, _y: f32) -> Vec<Self::HitTarget> {
        if let Some(hit_node_ids) = &self.hit_node_ids {
            return self
                .hits
                .iter()
                .filter(|target| hit_node_ids.contains(&target.node_id))
                .cloned()
                .collect();
        }
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
struct AppContextProbeHitTarget {
    node_id: cranpose_core::NodeId,
    densities: Rc<RefCell<Vec<f32>>>,
}

impl HitTestTarget for AppContextProbeHitTarget {
    fn dispatch(&self, _event: PointerEvent) {
        self.densities
            .borrow_mut()
            .push(cranpose_ui::current_density());
    }

    fn node_id(&self) -> cranpose_core::NodeId {
        self.node_id
    }

    fn capture_path(&self) -> Vec<cranpose_core::NodeId> {
        vec![self.node_id]
    }
}

struct AppContextProbeScene {
    target: AppContextProbeHitTarget,
}

impl RenderScene for AppContextProbeScene {
    type HitTarget = AppContextProbeHitTarget;

    fn clear(&mut self) {}

    fn hit_test(&self, _x: f32, _y: f32) -> Vec<Self::HitTarget> {
        vec![self.target.clone()]
    }

    fn find_target(&self, node_id: cranpose_core::NodeId) -> Option<Self::HitTarget> {
        (node_id == self.target.node_id).then(|| self.target.clone())
    }
}

macro_rules! renderer_scene_accessors {
    () => {
        fn scene(&self) -> &Self::Scene {
            &self.scene
        }

        fn scene_mut(&mut self) -> &mut Self::Scene {
            &mut self.scene
        }
    };
}

macro_rules! renderer_noop_rebuild {
    () => {
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
    };
}

struct AppContextProbeRenderer {
    scene: AppContextProbeScene,
}

impl Renderer for AppContextProbeRenderer {
    type Scene = AppContextProbeScene;
    type Error = ();

    renderer_scene_accessors!();
    renderer_noop_rebuild!();
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

struct FixedWidthTextMeasurer(f32);

impl cranpose_ui::TextMeasurer for FixedWidthTextMeasurer {
    fn measure(
        &self,
        _text: &cranpose_ui::text::AnnotatedString,
        _style: &cranpose_ui::TextStyle,
    ) -> cranpose_ui::TextMetrics {
        cranpose_ui::TextMetrics {
            width: self.0,
            height: 1.0,
            line_height: 1.0,
            line_count: 1,
        }
    }

    fn get_offset_for_position(
        &self,
        _text: &cranpose_ui::text::AnnotatedString,
        _style: &cranpose_ui::TextStyle,
        _x: f32,
        _y: f32,
    ) -> usize {
        0
    }

    fn get_cursor_x_for_offset(
        &self,
        _text: &cranpose_ui::text::AnnotatedString,
        _style: &cranpose_ui::TextStyle,
        _offset: usize,
    ) -> f32 {
        self.0
    }

    fn layout(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        _style: &cranpose_ui::TextStyle,
    ) -> cranpose_ui::TextLayoutResult {
        cranpose_ui::TextLayoutResult::monospaced(&text.text, self.0, 1.0)
    }
}

struct CountingFixedWidthTextMeasurer {
    width: f32,
    measure_calls: Rc<Cell<usize>>,
}

impl CountingFixedWidthTextMeasurer {
    fn new(width: f32, measure_calls: Rc<Cell<usize>>) -> Self {
        Self {
            width,
            measure_calls,
        }
    }
}

impl cranpose_ui::TextMeasurer for CountingFixedWidthTextMeasurer {
    fn measure(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::TextStyle,
    ) -> cranpose_ui::TextMetrics {
        self.measure_calls.set(self.measure_calls.get() + 1);
        FixedWidthTextMeasurer(self.width).measure(text, style)
    }

    fn get_offset_for_position(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::TextStyle,
        x: f32,
        y: f32,
    ) -> usize {
        FixedWidthTextMeasurer(self.width).get_offset_for_position(text, style, x, y)
    }

    fn get_cursor_x_for_offset(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::TextStyle,
        offset: usize,
    ) -> f32 {
        FixedWidthTextMeasurer(self.width).get_cursor_x_for_offset(text, style, offset)
    }

    fn layout(
        &self,
        text: &cranpose_ui::text::AnnotatedString,
        style: &cranpose_ui::TextStyle,
    ) -> cranpose_ui::TextLayoutResult {
        FixedWidthTextMeasurer(self.width).layout(text, style)
    }
}

#[derive(Default)]
struct TestRenderer {
    scene: TestScene,
    text_width: Option<f32>,
    text_measure_calls: Option<Rc<Cell<usize>>>,
}

impl TestRenderer {
    fn with_text_width(width: f32) -> Self {
        Self {
            scene: TestScene,
            text_width: Some(width),
            text_measure_calls: None,
        }
    }

    fn with_counting_text_width(width: f32, measure_calls: Rc<Cell<usize>>) -> Self {
        Self {
            scene: TestScene,
            text_width: Some(width),
            text_measure_calls: Some(measure_calls),
        }
    }
}

impl Renderer for TestRenderer {
    type Scene = TestScene;
    type Error = ();

    fn attach_app_context_services(&mut self, app_context: &cranpose_ui::AppContext) {
        if let Some(text_width) = self.text_width {
            if let Some(measure_calls) = self.text_measure_calls.clone() {
                app_context.set_text_measurer(CountingFixedWidthTextMeasurer::new(
                    text_width,
                    measure_calls,
                ));
            } else {
                app_context.set_text_measurer(FixedWidthTextMeasurer(text_width));
            }
        }
    }

    renderer_scene_accessors!();
    renderer_noop_rebuild!();
}

struct WarmupRenderer {
    scene: TestScene,
    needs_warmup: Rc<Cell<bool>>,
}

impl Renderer for WarmupRenderer {
    type Scene = TestScene;
    type Error = ();

    renderer_scene_accessors!();
    renderer_noop_rebuild!();

    fn needs_frame_warmup(&self) -> bool {
        self.needs_warmup.get()
    }
}

#[test]
fn renderer_warmup_keeps_frame_schedule_until_renderer_clears_it() {
    let _guard = test_guard();
    let needs_warmup = Rc::new(Cell::new(true));
    let mut shell = AppShell::new(
        WarmupRenderer {
            scene: TestScene,
            needs_warmup: Rc::clone(&needs_warmup),
        },
        location_key(file!(), line!(), column!()),
        || {},
    );

    shell.update();
    assert!(shell.needs_redraw());
    let warmup_schedule = shell.frame_schedule();
    assert!(
        !warmup_schedule.needs_update,
        "renderer warmup requests a frame, not UI update work"
    );
    assert!(warmup_schedule.needs_frame);

    needs_warmup.set(false);
    shell.update();
    assert!(!shell.needs_redraw());
    assert!(!shell.frame_schedule().needs_frame);
}

#[test]
fn idle_ui_task_wakes_for_an_update_without_scheduling_a_frame() {
    let _guard = test_guard();
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {},
    );

    shell.update();
    shell.runtime.runtime_handle().post_ui(|| {});

    let pending = shell.frame_schedule();
    assert!(pending.needs_update, "queued UI work must wake the shell");
    assert!(
        !pending.needs_frame,
        "a UI task that has not invalidated anything must not request a present"
    );

    assert_eq!(shell.update(), FrameUpdateResult::default());
    let settled = shell.frame_schedule();
    assert!(!settled.needs_update);
    assert!(!settled.needs_frame);
}

#[test]
fn a_bare_render_invalidation_re_presents_without_scene_work() {
    let _guard = test_guard();
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let visual_updates = Rc::new(Cell::new(0));
    let mut shell = AppShell::new(
        ScopedUpdateCountingRenderer::with_visual_updates(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::clone(&visual_updates),
            Rc::new(RefCell::new(Vec::new())),
        ),
        location_key(file!(), line!(), column!()),
        || {
            Text(
                "retained content".to_string(),
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );

    shell.set_viewport(320.0, 240.0);
    shell.update();
    while shell.frame_schedule().needs_update {
        shell.update();
    }
    assert!(rebuilds.get() > 0, "settling must have built the scene");
    shell.debug_enter_app_context(cranpose_ui::request_render_invalidation);
    shell.update();
    let settled = (rebuilds.get(), updates.get(), visual_updates.get());

    shell.debug_enter_app_context(cranpose_ui::request_render_invalidation);
    let result = shell.update();

    assert!(
        result.visual_changed,
        "a frame request must still present the retained scene"
    );
    assert!(!result.structure_changed);
    assert_eq!(
        (rebuilds.get(), updates.get(), visual_updates.get()),
        settled,
        "a bare render invalidation must not rebuild or scoped-update the scene"
    );
}

#[test]
fn two_app_shells_do_not_share_density_or_render_invalidations() {
    let _guard = test_guard();
    reset_public_render_state_for_test();

    let mut first = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {},
    );
    let mut second = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {},
    );

    first.update();
    second.update();
    assert!(!first.needs_redraw());
    assert!(!second.needs_redraw());

    first.set_density(2.0);
    assert_eq!(first.debug_current_density(), 2.0);
    assert_eq!(second.debug_current_density(), 1.0);
    assert!(first.needs_redraw());
    assert!(first.frame_schedule().needs_frame);
    assert!(!second.needs_redraw());
    assert!(!second.frame_schedule().needs_frame);

    first.update();
    assert!(!first.needs_redraw());
    assert!(!first.frame_schedule().needs_frame);

    first.set_frame_pacing_mode(FramePacingMode::Hard60);
    assert!(first.needs_redraw());
    assert!(first.frame_schedule().needs_frame);
    assert!(!second.needs_redraw());
}

#[test]
fn app_shell_font_scale_is_per_app_context_and_asks_for_a_frame() {
    let _guard = test_guard();
    let mut first = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {},
    );
    let mut second = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {},
    );

    first.update();
    second.update();
    assert_eq!(first.debug_current_font_scale(), 1.0);

    first.set_font_scale(1.3);
    assert_eq!(first.debug_current_font_scale(), 1.3);
    assert_eq!(second.debug_current_font_scale(), 1.0);
    assert!(
        first.needs_redraw(),
        "text sizes changed and nothing redrew"
    );
    assert!(!second.needs_redraw());

    first.update();
    assert!(!first.needs_redraw());

    first.set_font_scale(1.3);
    assert!(!first.needs_redraw());

    first.set_font_scale(0.0);
    assert_eq!(first.debug_current_font_scale(), 1.0);
    first.set_font_scale(f32::NAN);
    assert_eq!(first.debug_current_font_scale(), 1.0);
    first.set_font_scale(99.0);
    assert_eq!(
        first.debug_current_font_scale(),
        cranpose_ui::MAX_FONT_SCALE
    );
}

#[test]
fn app_shell_initial_composition_uses_constructor_density() {
    let _guard = test_guard();
    APP_SHELL_INITIAL_DENSITIES.with(|densities| densities.borrow_mut().clear());

    let shell = AppShell::new_with_size_and_density(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        AppShellCaptureInitialDensity,
        (1600, 900),
        (800.0, 450.0),
        2.0,
    );

    assert_eq!(shell.debug_current_density(), 2.0);
    let observed = APP_SHELL_INITIAL_DENSITIES.with(|densities| densities.borrow().clone());
    assert!(
        !observed.is_empty(),
        "initial composition should read density during shell construction"
    );
    assert!(
        observed.iter().all(|density| *density == 2.0),
        "initial composition densities should all use constructor density, got {observed:?}"
    );
}

#[test]
fn app_shell_scene_rebuilds_after_backdrop_effect_state_change() {
    let _guard = test_guard();
    APP_SHELL_BACKDROP_RADIUS_STATE.with(|slot| {
        slot.borrow_mut().take();
    });
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_backdrop_radius_content,
    );

    shell.update();
    let initial_radii = graph_backdrop_blur_radii(
        &shell
            .scene()
            .graph
            .as_ref()
            .expect("initial graph should be built")
            .root,
    );
    assert_eq!(initial_radii, vec![0.0]);

    let radius = APP_SHELL_BACKDROP_RADIUS_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("radius state should be registered");
    radius.set_value(18.0);
    shell.update();

    let updated_radii = graph_backdrop_blur_radii(
        &shell
            .scene()
            .graph
            .as_ref()
            .expect("updated graph should be built")
            .root,
    );
    assert_eq!(updated_radii, vec![18.0]);
}

#[test]
fn two_app_shells_do_not_share_fps_stats() {
    let _guard = test_guard();
    reset_public_render_state_for_test();

    let mut first = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {},
    );
    let second = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {},
    );

    let first_start = first.fps_stats().frame_count;
    let second_start = second.fps_stats().frame_count;

    first.record_presented_frame_for_test(16_000_000, 18_000_000);
    first.record_presented_frame_for_test(32_000_000, 34_000_000);

    assert_eq!(first.fps_stats().frame_count, first_start + 2);
    assert_eq!(second.fps_stats().frame_count, second_start);
}

#[test]
fn app_shell_idle_updates_do_not_advance_presented_frame_stats() {
    let _guard = test_guard();
    reset_public_render_state_for_test();

    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {},
    );

    let frame_count = shell.fps_stats().frame_count;
    shell.update_at_frame_time_nanos(16_000_000);
    shell.update_at_frame_time_nanos(32_000_000);

    assert_eq!(
        shell.fps_stats().frame_count,
        frame_count,
        "AppShell update work is not a presented redraw and must not mutate FPS stats"
    );
}

#[test]
fn pointer_event_clock_supports_realtime_and_exact_sampling() {
    let _guard = test_guard();
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {},
    );

    shell.update_at_frame_time_nanos(120_000_000);
    assert_eq!(
        shell.exact_pointer_event_time(Some(17)),
        PointerEventTime {
            platform_time_ms: Some(17),
            animation_time_nanos: 120_000_000,
        }
    );

    let realtime = shell.realtime_pointer_event_time(Some(18));
    assert_eq!(realtime.platform_time_ms, Some(18));
    assert!(realtime.animation_time_nanos >= 120_000_000);

    shell.update_after_exact_interval(Duration::from_millis(8));
    assert_eq!(
        shell.exact_pointer_event_time(None).animation_time_nanos,
        128_000_000
    );
}

#[test]
fn two_app_shells_do_not_share_text_measurers() {
    let _guard = test_guard();
    reset_public_render_state_for_test();

    let first = AppShell::new(
        TestRenderer::with_text_width(11.0),
        location_key(file!(), line!(), column!()),
        || {},
    );
    let second = AppShell::new(
        TestRenderer::with_text_width(29.0),
        location_key(file!(), line!(), column!()),
        || {},
    );
    let text = cranpose_ui::text::AnnotatedString::from("same text");
    let style = cranpose_ui::TextStyle::default();

    let first_width =
        first.debug_enter_app_context(|| cranpose_ui::measure_text(&text, &style).width);
    let second_width =
        second.debug_enter_app_context(|| cranpose_ui::measure_text(&text, &style).width);
    let first_width_again =
        first.debug_enter_app_context(|| cranpose_ui::measure_text(&text, &style).width);

    assert_eq!(first_width, 11.0);
    assert_eq!(second_width, 29.0);
    assert_eq!(first_width_again, 11.0);
}

#[test]
fn two_app_shells_have_independent_text_caches() {
    let _guard = test_guard();
    reset_public_render_state_for_test();

    let first_calls = Rc::new(Cell::new(0));
    let second_calls = Rc::new(Cell::new(0));
    let first = AppShell::new(
        TestRenderer::with_counting_text_width(11.0, Rc::clone(&first_calls)),
        location_key(file!(), line!(), column!()),
        || {},
    );
    let second = AppShell::new(
        TestRenderer::with_counting_text_width(29.0, Rc::clone(&second_calls)),
        location_key(file!(), line!(), column!()),
        || {},
    );
    let text = cranpose_ui::text::AnnotatedString::from("same cached text");
    let style = cranpose_ui::TextStyle::default();

    let first_width =
        first.debug_enter_app_context(|| cranpose_ui::measure_text(&text, &style).width);
    let first_width_again =
        first.debug_enter_app_context(|| cranpose_ui::measure_text(&text, &style).width);
    let second_width =
        second.debug_enter_app_context(|| cranpose_ui::measure_text(&text, &style).width);
    let second_width_again =
        second.debug_enter_app_context(|| cranpose_ui::measure_text(&text, &style).width);
    let first_width_after_second =
        first.debug_enter_app_context(|| cranpose_ui::measure_text(&text, &style).width);

    assert_eq!(first_width, 11.0);
    assert_eq!(first_width_again, 11.0);
    assert_eq!(second_width, 29.0);
    assert_eq!(second_width_again, 29.0);
    assert_eq!(first_width_after_second, 11.0);
    assert_eq!(
        first_calls.get(),
        1,
        "first shell should reuse its own text cache and not be invalidated by measuring in the second shell"
    );
    assert_eq!(
        second_calls.get(),
        1,
        "second shell should populate and reuse an independent text cache"
    );
}

#[test]
fn pointer_dispatch_enters_shell_app_context() {
    let _guard = test_guard();
    reset_public_render_state_for_test();

    let densities = Rc::new(RefCell::new(Vec::new()));
    let scene = AppContextProbeScene {
        target: AppContextProbeHitTarget {
            node_id: 1,
            densities: densities.clone(),
        },
    };
    let renderer = AppContextProbeRenderer { scene };
    let mut shell = AppShell::new(renderer, location_key(file!(), line!(), column!()), || {});

    shell.set_density(2.5);
    assert!(shell.set_cursor(12.0, 24.0));

    let observed = densities.borrow();
    assert!(
        !observed.is_empty(),
        "pointer dispatch should reach the probe target"
    );
    assert!(
        observed
            .iter()
            .all(|density| density.to_bits() == 2.5f32.to_bits()),
        "pointer dispatch observed densities {observed:?}",
    );
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

    renderer_scene_accessors!();
    renderer_noop_rebuild!();
}

impl Renderer for ScrollDispatchRenderer {
    type Scene = RecordingScene;
    type Error = ();

    renderer_scene_accessors!();
    renderer_noop_rebuild!();
}

#[derive(Default)]
struct RecordingRenderer {
    scene: TestScene,
    last_scene: Option<cranpose_ui::RecordedRenderScene>,
}

impl Renderer for RecordingRenderer {
    type Scene = TestScene;
    type Error = ();

    renderer_scene_accessors!();

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
    overlay_texts: Rc<RefCell<Vec<String>>>,
}

impl CountingRenderer {
    fn new(rebuilds: Rc<Cell<usize>>) -> Self {
        Self::with_overlay_texts(rebuilds, Rc::new(RefCell::new(Vec::new())))
    }

    fn with_overlay_texts(
        rebuilds: Rc<Cell<usize>>,
        overlay_texts: Rc<RefCell<Vec<String>>>,
    ) -> Self {
        Self {
            scene: TestScene,
            rebuilds,
            overlay_texts,
        }
    }
}

impl Renderer for CountingRenderer {
    type Scene = TestScene;
    type Error = ();

    renderer_scene_accessors!();

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

    fn draw_dev_overlay(&mut self, text: &str, _viewport: Size) {
        self.overlay_texts.borrow_mut().push(text.to_string());
    }
}

struct ScopedUpdateCountingRenderer {
    scene: cranpose_render_common::graph_scene::Scene,
    rebuilds: Rc<Cell<usize>>,
    updates: Rc<Cell<usize>>,
    visual_updates: Rc<Cell<usize>>,
    last_dirty_nodes: Rc<RefCell<Vec<cranpose_core::NodeId>>>,
}

impl ScopedUpdateCountingRenderer {
    fn new(
        rebuilds: Rc<Cell<usize>>,
        updates: Rc<Cell<usize>>,
        last_dirty_nodes: Rc<RefCell<Vec<cranpose_core::NodeId>>>,
    ) -> Self {
        Self::with_visual_updates(rebuilds, updates, Rc::new(Cell::new(0)), last_dirty_nodes)
    }

    fn with_visual_updates(
        rebuilds: Rc<Cell<usize>>,
        updates: Rc<Cell<usize>>,
        visual_updates: Rc<Cell<usize>>,
        last_dirty_nodes: Rc<RefCell<Vec<cranpose_core::NodeId>>>,
    ) -> Self {
        Self {
            scene: cranpose_render_common::graph_scene::Scene::default(),
            rebuilds,
            updates,
            visual_updates,
            last_dirty_nodes,
        }
    }
}

impl Renderer for ScopedUpdateCountingRenderer {
    type Scene = cranpose_render_common::graph_scene::Scene;
    type Error = ();

    renderer_scene_accessors!();

    fn rebuild_scene(
        &mut self,
        layout_tree: &LayoutTree,
        _viewport: Size,
    ) -> Result<(), Self::Error> {
        self.rebuilds.set(self.rebuilds.get() + 1);
        self.scene.clear();
        let graph = cranpose_render_common::scene_builder::build_graph_from_layout_tree(
            layout_tree.root(),
            1.0,
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
        self.rebuilds.set(self.rebuilds.get() + 1);
        self.scene.clear();
        if let Some(graph) =
            cranpose_render_common::scene_builder::build_graph_from_applier(applier, root, 1.0)
        {
            self.scene.replace_graph(graph);
        }
        Ok(())
    }

    fn update_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
        dirty_nodes: &[cranpose_core::NodeId],
    ) -> Result<(), Self::Error> {
        self.updates.set(self.updates.get() + 1);
        *self.last_dirty_nodes.borrow_mut() = dirty_nodes.to_vec();
        let updated = self.scene.graph.as_mut().is_some_and(|graph| {
            cranpose_render_common::scene_builder::update_graph_from_applier(
                applier,
                graph,
                dirty_nodes,
                1.0,
            )
        });
        if !updated {
            self.rebuilds.set(self.rebuilds.get() + 1);
            self.scene.clear();
            if let Some(graph) =
                cranpose_render_common::scene_builder::build_graph_from_applier(applier, root, 1.0)
            {
                self.scene.replace_graph(graph);
            }
        }
        Ok(())
    }

    fn update_visual_scene_from_applier(
        &mut self,
        applier: &mut cranpose_core::MemoryApplier,
        root: cranpose_core::NodeId,
        _viewport: Size,
        dirty_nodes: &[cranpose_core::NodeId],
    ) -> Result<(), Self::Error> {
        self.visual_updates.set(self.visual_updates.get() + 1);
        *self.last_dirty_nodes.borrow_mut() = dirty_nodes.to_vec();
        let updated = self.scene.graph.as_mut().is_some_and(|graph| {
            cranpose_render_common::scene_builder::update_graph_from_applier(
                applier,
                graph,
                dirty_nodes,
                1.0,
            )
        });
        if !updated {
            self.rebuilds.set(self.rebuilds.get() + 1);
            self.scene.clear();
            if let Some(graph) =
                cranpose_render_common::scene_builder::build_graph_from_applier(applier, root, 1.0)
            {
                self.scene.replace_graph(graph);
            }
        }
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

    renderer_scene_accessors!();

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

thread_local! {
    static APP_SHELL_BACKDROP_RADIUS_STATE: RefCell<Option<MutableState<f32>>> =
        const { RefCell::new(None) };
}

#[composable]
fn app_shell_backdrop_radius_content() {
    let radius = rememberMutableStateOf(|| 0.0f32);
    APP_SHELL_BACKDROP_RADIUS_STATE.with(|slot| {
        *slot.borrow_mut() = Some(radius);
    });

    Box(
        Modifier::empty()
            .size_points(128.0, 96.0)
            .draw_behind(|scope| {
                scope.draw_rect(Brush::solid(Color::from_rgba_u8(30, 40, 60, 255)));
            }),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .absolute_offset(16.0, 16.0)
                    .size_points(96.0, 56.0)
                    .graphics_layer_value(GraphicsLayer {
                        render_effect: Some(RenderEffect::blur(0.0)),
                        ..GraphicsLayer::default()
                    }),
                BoxSpec::default(),
                move || {
                    Box(
                        Modifier::empty()
                            .absolute_offset(40.0, 12.0)
                            .size_points(42.0, 30.0)
                            .backdrop_effect(RenderEffect::blur(radius.get())),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}

fn graph_backdrop_blur_radii(layer: &cranpose_render_common::graph::LayerNode) -> Vec<f32> {
    fn collect(layer: &cranpose_render_common::graph::LayerNode, out: &mut Vec<f32>) {
        if let Some(RenderEffect::Blur { radius_x, .. }) = &layer.graphics_layer.backdrop_effect {
            out.push(*radius_x);
        }
        for child in &layer.children {
            if let cranpose_render_common::graph::RenderNode::Layer(child_layer) = child {
                collect(child_layer, out);
            }
        }
    }

    let mut radii = Vec::new();
    collect(layer, &mut radii);
    radii
}

#[composable]
fn tabbed_progress_content() {
    let progress = rememberMutableStateOf(|| 0.6f32);
    let active_tab = rememberMutableStateOf(|| 0i32);

    let progress_effect = progress;
    let active_effect = active_tab;
    launched_effect_async_impl(
        location_key(file!(), line!(), column!()),
        TaskSite::new(file!(), line!()),
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
fn one_shot_frame_request_content() {
    let phase = rememberMutableStateOf(|| 0i32);
    let phase_state = phase;
    launched_effect_async_impl(
        location_key(file!(), line!(), column!()),
        TaskSite::new(file!(), line!()),
        (),
        move |scope| {
            let phase = phase_state;
            Box::pin(async move {
                let clock = scope.runtime().frame_clock();
                let _ = clock.next_frame().await;
                phase.set_value(1);
            })
        },
    );

    Text(
        if phase.value() == 0 {
            "Waiting For Frame"
        } else {
            "Frame Applied"
        },
        Modifier::empty(),
        TextStyle::default(),
    );
}

#[composable]
fn silent_frame_loop_content() {
    launched_effect_async_impl(
        location_key(file!(), line!(), column!()),
        TaskSite::new(file!(), line!()),
        (),
        move |scope| {
            Box::pin(async move {
                let clock = scope.runtime().frame_clock();
                while scope.is_active() {
                    let _ = clock.next_frame().await;
                }
            })
        },
    );

    Text("Still", Modifier::empty(), TextStyle::default());
}

#[composable]
fn continuous_frame_request_tab() {
    let tick = rememberMutableStateOf(|| 0u32);
    let tick_state = tick;
    launched_effect_async_impl(
        location_key(file!(), line!(), column!()),
        TaskSite::new(file!(), line!()),
        (),
        move |scope| {
            let tick = tick_state;
            Box::pin(async move {
                let clock = scope.runtime().frame_clock();
                while scope.is_active() {
                    let _ = clock.next_frame().await;
                    if !scope.is_active() {
                        break;
                    }
                    APP_SHELL_CONTINUOUS_FRAME_COUNT
                        .with(|count| count.set(count.get().saturating_add(1)));
                    tick.update(|value| *value = value.wrapping_add(1));
                }
            })
        },
    );

    Text(
        format!("Animated tick {}", tick.value()),
        Modifier::empty(),
        TextStyle::default(),
    );
}

#[composable]
fn app_shell_continuous_then_static_tab_host() {
    let active = rememberMutableStateOf(|| 0i32);
    APP_SHELL_ACTIVE_TAB_STATE.with(|slot| {
        *slot.borrow_mut() = Some(active);
    });

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            if active.value() == 0 {
                continuous_frame_request_tab();
            } else {
                Text("Static tab", Modifier::empty(), TextStyle::default());
            }
        },
    );
}

#[composable]
fn frame_time_recorder_content() {
    launched_effect_async_impl(
        location_key(file!(), line!(), column!()),
        TaskSite::new(file!(), line!()),
        (),
        move |scope| {
            Box::pin(async move {
                let clock = scope.runtime().frame_clock();
                let first = clock.next_frame().await;
                APP_SHELL_FRAME_TIME_RECORDS.with(|records| records.borrow_mut().push(first));
                let second = clock.next_frame().await;
                APP_SHELL_FRAME_TIME_RECORDS.with(|records| records.borrow_mut().push(second));
            })
        },
    );

    Text(
        "Frame Time Recorder",
        Modifier::empty(),
        TextStyle::default(),
    );
}

#[composable]
fn frame_stable_pointer_handler_content() {
    let use_pending_handler = rememberMutableStateOf(|| false);
    let rendered_clicks = rememberMutableStateOf(|| 0i32);
    let pending_clicks = rememberMutableStateOf(|| 0i32);
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
        ButtonSpec::default(),
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

fn draw_observed_width_app(width_state: cranpose_core::MutableState<f32>) {
    Box(
        Modifier::empty()
            .size(Size {
                width: 200.0,
                height: 40.0,
            })
            .draw_behind(move |scope| {
                let width = width_state.get();
                scope.draw_rect_at(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width,
                        height: 10.0,
                    },
                    Brush::solid(Color(0.2, 0.7, 0.3, 1.0)),
                );
            }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn graphics_layer_observed_offset_app(offset_state: cranpose_core::MutableState<f32>) {
    Box(
        Modifier::empty()
            .size(Size {
                width: 200.0,
                height: 40.0,
            })
            .graphics_layer({
                move || GraphicsLayer {
                    translation_x: offset_state.get(),
                    ..Default::default()
                }
            })
            .draw_behind(|scope| {
                scope.draw_rect_at(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 40.0,
                        height: 20.0,
                    },
                    Brush::solid(Color(0.4, 0.5, 0.9, 1.0)),
                );
            }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn graphics_layer_composed_offset_app(offset_state: cranpose_core::MutableState<f32>) {
    let offset = offset_state.get();
    Box(
        Modifier::empty()
            .size(Size {
                width: 200.0,
                height: 40.0,
            })
            .graphics_layer(move || GraphicsLayer {
                translation_x: offset,
                ..Default::default()
            })
            .draw_behind(|scope| {
                scope.draw_rect_at(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 40.0,
                        height: 20.0,
                    },
                    Brush::solid(Color(0.4, 0.5, 0.9, 1.0)),
                );
            }),
        BoxSpec::default(),
        || {},
    );
}

fn shader_rect_like_effect(seed: usize, time: f32, intensity: f32) -> RenderEffect {
    let mut shader = RuntimeShader::new("shader-rect-scoped-update-test");
    shader.set_float(0, seed as f32);
    shader.set_float(1, time);
    shader.set_float(2, intensity);
    RenderEffect::runtime_shader(shader)
}

#[composable]
fn shader_rect_like_effect_layers_app(
    time_state: cranpose_core::MutableState<f32>,
    intensity_state: cranpose_core::MutableState<f32>,
) {
    let time = time_state.get();
    let intensity = intensity_state.get();

    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            for row in 0..2 {
                Row(
                    Modifier::empty().fill_max_width(),
                    RowSpec::default(),
                    move || {
                        for col in 0..2 {
                            let seed = row * 2 + col;
                            let effect = shader_rect_like_effect(seed, time, intensity);
                            Box(
                                Modifier::empty()
                                    .size(Size {
                                        width: 96.0,
                                        height: 48.0,
                                    })
                                    .graphics_layer({
                                        let effect = effect.clone();
                                        move || GraphicsLayer {
                                            render_effect: Some(effect.clone()),
                                            compositing_strategy: CompositingStrategy::Offscreen,
                                            ..Default::default()
                                        }
                                    })
                                    .draw_behind(move |scope| {
                                        scope.draw_rect_at(
                                            Rect {
                                                x: 0.0,
                                                y: 0.0,
                                                width: 96.0,
                                                height: 48.0,
                                            },
                                            Brush::solid(Color(
                                                0.1 + seed as f32 * 0.05,
                                                0.2,
                                                0.4,
                                                1.0,
                                            )),
                                        );
                                    }),
                                BoxSpec::new().content_alignment(Alignment::CENTER),
                                move || {
                                    Text(
                                        format!("Shader {seed}"),
                                        Modifier::empty(),
                                        TextStyle::default(),
                                    );
                                },
                            );
                        }
                    },
                );
            }
        },
    );
}

#[composable]
fn shader_rect_like_lazy_effect_layers_app(
    time_state: cranpose_core::MutableState<f32>,
    intensity_state: cranpose_core::MutableState<f32>,
) {
    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            for row in 0..2 {
                Row(
                    Modifier::empty().fill_max_width(),
                    RowSpec::default(),
                    move || {
                        for col in 0..2 {
                            let seed = row * 2 + col;
                            Box(
                                Modifier::empty()
                                    .size(Size {
                                        width: 96.0,
                                        height: 48.0,
                                    })
                                    .graphics_layer(move || {
                                        let effect = shader_rect_like_effect(
                                            seed,
                                            time_state.get(),
                                            intensity_state.get(),
                                        );
                                        GraphicsLayer {
                                            render_effect: Some(effect),
                                            compositing_strategy: CompositingStrategy::Offscreen,
                                            ..Default::default()
                                        }
                                    })
                                    .draw_behind(move |scope| {
                                        scope.draw_rect_at(
                                            Rect {
                                                x: 0.0,
                                                y: 0.0,
                                                width: 96.0,
                                                height: 48.0,
                                            },
                                            Brush::solid(Color(
                                                0.1 + seed as f32 * 0.05,
                                                0.2,
                                                0.4,
                                                1.0,
                                            )),
                                        );
                                    }),
                                BoxSpec::new().content_alignment(Alignment::CENTER),
                                move || {
                                    Text(
                                        format!("Shader {seed}"),
                                        Modifier::empty(),
                                        TextStyle::default(),
                                    );
                                },
                            );
                        }
                    },
                );
            }
        },
    );
}

#[composable]
fn graphics_layer_observed_point_app(position_state: cranpose_core::MutableState<Point>) {
    Box(
        Modifier::empty()
            .size(Size {
                width: 200.0,
                height: 40.0,
            })
            .graphics_layer({
                move || {
                    let position = position_state.get();
                    GraphicsLayer {
                        translation_x: position.x,
                        translation_y: position.y,
                        ..Default::default()
                    }
                }
            })
            .draw_behind(|scope| {
                scope.draw_rect_at(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: 40.0,
                        height: 20.0,
                    },
                    Brush::solid(Color(0.4, 0.5, 0.9, 1.0)),
                );
            }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn pointer_driven_graphics_layer_point_app(position_state: cranpose_core::MutableState<Point>) {
    Box(
        Modifier::empty()
            .size(Size {
                width: 320.0,
                height: 220.0,
            })
            .background(Color(0.08, 0.10, 0.14, 1.0)),
        BoxSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .size(Size {
                        width: 100.0,
                        height: 80.0,
                    })
                    .graphics_layer({
                        move || {
                            let position = position_state.get();
                            GraphicsLayer {
                                translation_x: position.x,
                                translation_y: position.y,
                                ..Default::default()
                            }
                        }
                    })
                    .draw_behind(|scope| {
                        scope.draw_rect_at(
                            Rect {
                                x: 0.0,
                                y: 0.0,
                                width: 100.0,
                                height: 80.0,
                            },
                            Brush::solid(Color(0.4, 0.5, 0.9, 1.0)),
                        );
                    })
                    .pointer_input((), {
                        move |scope: PointerInputScope| async move {
                            scope
                                .await_pointer_event_scope(|await_scope| async move {
                                    loop {
                                        let event = await_scope.await_pointer_event().await;
                                        match event.kind {
                                            PointerEventKind::Down | PointerEventKind::Move => {
                                                position_state.set(event.position);
                                                event.consume();
                                            }
                                            PointerEventKind::Up
                                            | PointerEventKind::Cancel
                                            | PointerEventKind::Scroll
                                            | PointerEventKind::Zoom
                                            | PointerEventKind::RotaryScrollPre
                                            | PointerEventKind::RotaryScroll
                                            | PointerEventKind::Enter
                                            | PointerEventKind::Exit => {}
                                        }
                                    }
                                })
                                .await;
                        }
                    }),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[derive(Default)]
struct TextFieldDispatchProbe {
    pasted_text: RefCell<Option<String>>,
    cut_in_event_handler: Cell<bool>,
    cut_in_applied_snapshot: Cell<bool>,
    paste_in_event_handler: Cell<bool>,
    paste_in_applied_snapshot: Cell<bool>,
    preedit_text: RefCell<Option<(String, Option<(usize, usize)>)>>,
    preedit_in_event_handler: Cell<bool>,
    preedit_in_applied_snapshot: Cell<bool>,
    last_delete: Cell<Option<(usize, usize)>>,
    delete_in_event_handler: Cell<bool>,
    delete_in_applied_snapshot: Cell<bool>,
    finish_composition_count: Cell<usize>,
    last_composing_region: Cell<Option<(usize, usize)>>,
    last_selection: Cell<Option<(usize, usize)>>,
}

impl cranpose_ui::text_field_focus::FocusedTextFieldHandler for TextFieldDispatchProbe {
    fn handle_key(&self, _event: &cranpose_ui::KeyEvent) -> bool {
        false
    }

    fn insert_text(&self, text: &str) {
        self.pasted_text.replace(Some(text.to_string()));
        self.paste_in_event_handler
            .set(cranpose_core::in_event_handler());
        self.paste_in_applied_snapshot
            .set(cranpose_core::in_applied_snapshot());
    }

    fn delete_surrounding(&self, before_bytes: usize, after_bytes: usize) {
        self.last_delete.set(Some((before_bytes, after_bytes)));
        self.delete_in_event_handler
            .set(cranpose_core::in_event_handler());
        self.delete_in_applied_snapshot
            .set(cranpose_core::in_applied_snapshot());
    }

    fn copy_selection(&self) -> Option<String> {
        None
    }

    fn cut_selection(&self) -> Option<String> {
        self.cut_in_event_handler
            .set(cranpose_core::in_event_handler());
        self.cut_in_applied_snapshot
            .set(cranpose_core::in_applied_snapshot());
        Some("cut text".to_string())
    }

    fn set_composition(&self, text: &str, cursor: Option<(usize, usize)>) {
        self.preedit_text.replace(Some((text.to_string(), cursor)));
        self.preedit_in_event_handler
            .set(cranpose_core::in_event_handler());
        self.preedit_in_applied_snapshot
            .set(cranpose_core::in_applied_snapshot());
    }

    fn finish_composition(&self) {
        self.finish_composition_count
            .set(self.finish_composition_count.get() + 1);
    }

    fn set_composing_region(&self, start_bytes: usize, end_bytes: usize) {
        self.last_composing_region
            .set(Some((start_bytes, end_bytes)));
    }

    fn set_selection(&self, start_bytes: usize, end_bytes: usize) {
        self.last_selection.set(Some((start_bytes, end_bytes)));
    }

    fn editor_state(&self) -> Option<cranpose_ui::ImeEditorState> {
        Some(cranpose_ui::ImeEditorState {
            text: "probe".to_string(),
            selection_start: 1,
            selection_end: 2,
            composition: Some((0, 3)),
            single_line: true,
        })
    }
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
        let live_slots = live_slot_count(&shell.debug_slot_entries());
        baseline_live_slots.get_or_insert(live_slots);
        peak_live_slots = peak_live_slots.max(live_slots);
    }

    let baseline_live_slots = baseline_live_slots.expect("baseline live slot count");
    assert!(
        peak_live_slots <= baseline_live_slots + 64,
        "tabbed progress updates leaked live slots: baseline={baseline_live_slots} peak={peak_live_slots}",
    );
}

#[derive(Default)]
struct SoftKeyboardProbe {
    calls: RefCell<Vec<&'static str>>,
}

impl cranpose_ui::PlatformTextInputHandler for SoftKeyboardProbe {
    fn show_keyboard(&self) {
        self.calls.borrow_mut().push("show");
    }

    fn hide_keyboard(&self) {
        self.calls.borrow_mut().push("hide");
    }
}

#[test]
fn text_field_focus_drives_platform_soft_keyboard() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, empty_content);
    shell.update();

    let keyboard = Rc::new(SoftKeyboardProbe::default());
    shell.set_platform_text_input(keyboard.clone());

    let focus_flag = Rc::new(RefCell::new(false));
    let handler = Rc::new(TextFieldDispatchProbe::default());

    shell.debug_enter_app_context(|| {
        cranpose_ui::text_field_focus::request_focus(Rc::clone(&focus_flag), handler.clone(), 0);
    });
    assert_eq!(*keyboard.calls.borrow(), vec!["show"]);

    shell.debug_enter_app_context(cranpose_ui::text_field_focus::clear_focus);
    assert_eq!(*keyboard.calls.borrow(), vec!["show", "hide"]);
}

#[test]
fn key_event_after_field_removal_hides_soft_keyboard() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, empty_content);
    shell.update();

    let keyboard = Rc::new(SoftKeyboardProbe::default());
    shell.set_platform_text_input(keyboard.clone());

    let handler = Rc::new(TextFieldDispatchProbe::default());
    {
        let focus_flag = Rc::new(RefCell::new(false));
        shell.debug_enter_app_context(|| {
            cranpose_ui::text_field_focus::request_focus(
                Rc::clone(&focus_flag),
                handler.clone(),
                0,
            );
        });
    }

    let event = KeyEvent::key_down(KeyCode::A, "a");
    assert!(!shell.on_key_event(&event));
    assert_eq!(*keyboard.calls.borrow(), vec!["show", "hide"]);
}

#[test]
fn frame_update_after_field_removal_hides_soft_keyboard() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, empty_content);
    shell.update();

    let keyboard = Rc::new(SoftKeyboardProbe::default());
    shell.set_platform_text_input(keyboard.clone());

    let handler = Rc::new(TextFieldDispatchProbe::default());
    {
        let focus_flag = Rc::new(RefCell::new(false));
        shell.debug_enter_app_context(|| {
            cranpose_ui::text_field_focus::request_focus(
                Rc::clone(&focus_flag),
                handler.clone(),
                0,
            );
        });
    }
    assert_eq!(*keyboard.calls.borrow(), vec!["show"]);

    shell.update();
    assert_eq!(*keyboard.calls.borrow(), vec!["show", "hide"]);
}

#[test]
fn ime_delete_surrounding_marks_dirty() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, empty_content);
    shell.update();
    assert!(!shell.needs_redraw());

    let focus_flag = Rc::new(RefCell::new(false));
    let handler = Rc::new(TextFieldDispatchProbe::default());

    shell.debug_enter_app_context(|| {
        cranpose_ui::text_field_focus::request_focus(Rc::clone(&focus_flag), handler.clone(), 0);
    });
    assert!(shell.on_ime_delete_surrounding(2, 1));
    assert_eq!(handler.last_delete.get(), Some((2, 1)));
    assert!(shell.needs_redraw());
    shell.debug_enter_app_context(cranpose_ui::text_field_focus::clear_focus);
}

#[test]
fn ime_session_shell_methods_dispatch_to_focused_field() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, empty_content);
    shell.update();

    let keyboard = Rc::new(SoftKeyboardProbe::default());
    shell.set_platform_text_input(keyboard.clone());

    assert!(!shell.on_ime_finish_composing());
    assert!(!shell.on_ime_set_composing_region(0, 1));
    assert!(!shell.on_ime_set_selection(0, 1));
    assert!(shell.ime_editor_state().is_none());

    let focus_flag = Rc::new(RefCell::new(false));
    let handler = Rc::new(TextFieldDispatchProbe::default());
    shell.debug_enter_app_context(|| {
        cranpose_ui::text_field_focus::request_focus(Rc::clone(&focus_flag), handler.clone(), 0);
    });
    assert_eq!(*keyboard.calls.borrow(), vec!["show"]);

    assert!(shell.on_ime_finish_composing());
    assert_eq!(handler.finish_composition_count.get(), 1);

    assert!(shell.on_ime_set_composing_region(2, 5));
    assert_eq!(handler.last_composing_region.get(), Some((2, 5)));

    assert!(shell.on_ime_set_selection(3, 3));
    assert_eq!(handler.last_selection.get(), Some((3, 3)));

    let state = shell.ime_editor_state().expect("focused editor state");
    assert_eq!(state.text, "probe");
    assert_eq!((state.selection_start, state.selection_end), (1, 2));
    assert_eq!(state.composition, Some((0, 3)));
    assert!(state.single_line);

    shell.clear_text_field_focus();
    assert!(!*focus_flag.borrow());
    assert_eq!(*keyboard.calls.borrow(), vec!["show", "hide"]);
    assert!(shell.ime_editor_state().is_none());
}

#[test]
fn text_mutation_platform_events_run_inside_event_and_applied_snapshot_scopes() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, empty_content);
    shell.update();

    let focus_flag = Rc::new(RefCell::new(false));
    let handler = Rc::new(TextFieldDispatchProbe::default());

    shell.debug_enter_app_context(|| {
        cranpose_ui::text_field_focus::request_focus(Rc::clone(&focus_flag), handler.clone(), 0);
    });

    assert!(shell.on_paste("hello"));
    assert_eq!(handler.pasted_text.borrow().as_deref(), Some("hello"));
    assert!(handler.paste_in_event_handler.get());
    assert!(handler.paste_in_applied_snapshot.get());

    assert_eq!(shell.on_cut().as_deref(), Some("cut text"));
    assert!(handler.cut_in_event_handler.get());
    assert!(handler.cut_in_applied_snapshot.get());

    assert!(shell.on_ime_preedit("preedit", Some((1, 4))));
    assert_eq!(
        handler.preedit_text.borrow().as_ref(),
        Some(&("preedit".to_string(), Some((1, 4))))
    );
    assert!(handler.preedit_in_event_handler.get());
    assert!(handler.preedit_in_applied_snapshot.get());

    assert!(shell.on_ime_delete_surrounding(2, 1));
    assert_eq!(handler.last_delete.get(), Some((2, 1)));
    assert!(handler.delete_in_event_handler.get());
    assert!(handler.delete_in_applied_snapshot.get());

    shell.debug_enter_app_context(cranpose_ui::text_field_focus::clear_focus);
}

#[test]
fn pending_layout_request_skips_clean_tree_without_forcing_measure() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, || {
        Text(
            "steady".to_string(),
            Modifier::empty(),
            TextStyle::default(),
        );
    });

    shell.scene_dirty = false;
    shell.layout_requested = true;
    shell.force_layout_pass = false;

    shell.run_layout_phase();

    assert!(
        !shell.scene_dirty,
        "clean trees should not trigger a fresh layout pass when the request is not forced",
    );
    assert!(!shell.layout_requested);
    assert!(!shell.force_layout_pass);
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
fn pointer_scrolled_dispatches_capture_path_ancestors() {
    let _guard = test_guard();
    let child_events = Rc::new(RefCell::new(Vec::new()));
    let ancestor_events = Rc::new(RefCell::new(Vec::new()));
    let scene = RecordingScene::with_hit_node_ids(
        vec![
            RecordingHitTarget {
                node_id: 1,
                consume: false,
                events: child_events.clone(),
                capture_path: vec![1, 99],
            },
            RecordingHitTarget {
                node_id: 99,
                consume: true,
                events: ancestor_events.clone(),
                capture_path: vec![99],
            },
        ],
        vec![1],
    );

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(ScrollDispatchRenderer::new(scene), root_key, empty_content);
    shell.set_cursor(20.0, 30.0);
    child_events.borrow_mut().clear();
    ancestor_events.borrow_mut().clear();

    let consumed = shell.pointer_scrolled(0.0, -42.0);
    assert!(
        consumed,
        "wheel dispatch should report ancestor scroll consumption"
    );
    assert_eq!(child_events.borrow().len(), 1);
    assert_eq!(
        ancestor_events.borrow().len(),
        1,
        "scroll must bubble through capture-path ancestors"
    );
}

#[test]
fn captured_gesture_cancels_when_original_targets_disappear() {
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
        !shell.set_cursor(30.0, 30.0),
        "move should cancel when no live captured node survives"
    );
    assert!(
        !shell.pointer_released(),
        "up should not replay detached handlers after the captured node disappears"
    );

    assert_eq!(
        first_target_events
            .borrow()
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![PointerEventKind::Down]
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
fn consumed_pointer_down_only_captures_targets_that_received_the_down_event() {
    let _guard = test_guard();
    let top_events = Rc::new(RefCell::new(Vec::new()));
    let lower_events = Rc::new(RefCell::new(Vec::new()));
    let active_hits = Rc::new(RefCell::new(vec![
        RecordingHitTarget {
            node_id: 1,
            consume: true,
            events: top_events.clone(),
            capture_path: vec![1],
        },
        RecordingHitTarget {
            node_id: 2,
            consume: false,
            events: lower_events.clone(),
            capture_path: vec![2],
        },
    ]));

    let root_key = location_key(file!(), line!(), column!());
    let scene = MutableRecordingScene::new(active_hits);
    let mut shell = AppShell::new(
        MutableRecordingRenderer::new(scene),
        root_key,
        empty_content,
    );

    assert!(
        shell.set_cursor(10.0, 10.0),
        "hover should find both overlapping hits"
    );
    top_events.borrow_mut().clear();
    lower_events.borrow_mut().clear();

    assert!(
        shell.pointer_pressed(),
        "pointer down should dispatch to the top-most hit"
    );
    assert_eq!(
        top_events
            .borrow()
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![PointerEventKind::Down]
    );
    assert!(
        lower_events.borrow().is_empty(),
        "covered hit must not receive Down once the top hit consumes it",
    );

    top_events.borrow_mut().clear();
    lower_events.borrow_mut().clear();

    assert!(
        shell.set_cursor(20.0, 20.0),
        "drag move should stay on the captured gesture path",
    );
    assert!(
        shell.pointer_released(),
        "pointer up should resolve through the captured gesture path",
    );

    assert_eq!(
        top_events
            .borrow()
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![PointerEventKind::Move, PointerEventKind::Up]
    );
    assert!(
        lower_events.borrow().is_empty(),
        "targets that never received Down must not receive Move or Up follow-ups",
    );
}

#[test]
fn captured_gesture_preserves_hit_order_when_paths_share_an_ancestor() {
    let _guard = test_guard();
    let first_child_events = Rc::new(RefCell::new(Vec::new()));
    let second_child_events = Rc::new(RefCell::new(Vec::new()));
    let shared_ancestor_events = Rc::new(RefCell::new(Vec::new()));
    let active_hits = Rc::new(RefCell::new(vec![
        RecordingHitTarget {
            node_id: 1,
            consume: false,
            events: first_child_events.clone(),
            capture_path: vec![1, 99],
        },
        RecordingHitTarget {
            node_id: 2,
            consume: false,
            events: second_child_events.clone(),
            capture_path: vec![2, 99],
        },
    ]));

    let root_key = location_key(file!(), line!(), column!());
    let scene = MutableRecordingScene::new(active_hits.clone());
    let mut shell = AppShell::new(
        MutableRecordingRenderer::new(scene),
        root_key,
        empty_content,
    );

    shell.set_cursor(10.0, 10.0);
    assert!(
        shell.pointer_pressed(),
        "down should record both overlapping hits"
    );

    first_child_events.borrow_mut().clear();
    second_child_events.borrow_mut().clear();
    shared_ancestor_events.borrow_mut().clear();
    *active_hits.borrow_mut() = vec![
        RecordingHitTarget {
            node_id: 1,
            consume: false,
            events: first_child_events.clone(),
            capture_path: vec![1, 99],
        },
        RecordingHitTarget {
            node_id: 2,
            consume: false,
            events: second_child_events.clone(),
            capture_path: vec![2, 99],
        },
        RecordingHitTarget {
            node_id: 99,
            consume: true,
            events: shared_ancestor_events.clone(),
            capture_path: vec![99],
        },
    ];

    assert!(
        shell.set_cursor(20.0, 20.0),
        "move should resolve the live capture tree"
    );
    assert!(
        shell.pointer_released(),
        "up should dispatch through the merged capture tree"
    );

    assert_eq!(
        first_child_events
            .borrow()
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![PointerEventKind::Move, PointerEventKind::Up]
    );
    assert_eq!(
        second_child_events
            .borrow()
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![PointerEventKind::Move, PointerEventKind::Up]
    );
    assert_eq!(
        shared_ancestor_events
            .borrow()
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![PointerEventKind::Move, PointerEventKind::Up]
    );
}

#[test]
fn captured_gesture_release_reaches_ancestor_even_when_child_consumes() {
    let _guard = test_guard();
    let child_events = Rc::new(RefCell::new(Vec::new()));
    let ancestor_events = Rc::new(RefCell::new(Vec::new()));
    let active_hits = Rc::new(RefCell::new(vec![RecordingHitTarget {
        node_id: 1,
        consume: true,
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
    assert!(shell.pointer_pressed(), "down should hit the child target");

    child_events.borrow_mut().clear();
    *active_hits.borrow_mut() = vec![
        RecordingHitTarget {
            node_id: 1,
            consume: true,
            events: child_events.clone(),
            capture_path: vec![1, 99],
        },
        RecordingHitTarget {
            node_id: 99,
            consume: false,
            events: ancestor_events.clone(),
            capture_path: vec![99],
        },
    ];

    assert!(
        shell.pointer_released(),
        "up should resolve the captured path"
    );

    assert_eq!(
        child_events
            .borrow()
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![PointerEventKind::Up]
    );
    assert_eq!(
        ancestor_events
            .borrow()
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![PointerEventKind::Up]
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
fn pointer_scrolled_reaches_real_vertical_scroll_modifier() {
    let _guard = test_guard();
    APP_SHELL_WHEEL_SCROLL_STATE.with(|slot| slot.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_wheel_scroll_probe,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    shell.update();

    let scroll_state = APP_SHELL_WHEEL_SCROLL_STATE
        .with(|slot| slot.borrow().as_ref().cloned())
        .expect("wheel scroll probe should expose its scroll state");
    assert_eq!(scroll_state.value_non_reactive(), 0.0);

    assert!(shell.set_cursor(80.0, 80.0), "probe should be hit-testable");
    assert!(
        shell.pointer_scrolled(0.0, -120.0),
        "wheel event should be consumed by the scroll modifier"
    );
    shell.update();

    assert!(
        scroll_state.value_non_reactive() > 0.0,
        "wheel event did not update vertical_scroll state"
    );
    assert!(
        !shell.needs_redraw(),
        "wheel scroll must not leave a redraw tail after the frame that applied the scroll"
    );
}

#[test]
fn wheel_scroll_updates_vertical_scroll_layout_tree_offset() {
    let _guard = test_guard();
    APP_SHELL_WHEEL_SCROLL_STATE.with(|slot| slot.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_wheel_scroll_probe,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    shell.update();

    let initial_bottom_y = find_layout_box_with_text(
        shell.layout_tree().expect("initial layout tree").root(),
        "Wheel scroll probe bottom",
    )
    .expect("bottom text in initial layout tree")
    .rect
    .y;

    assert!(shell.set_cursor(80.0, 80.0), "probe should be hit-testable");
    assert!(
        shell.pointer_scrolled(0.0, -120.0),
        "wheel event should be consumed by the scroll modifier"
    );
    shell.update();

    let scroll_offset = APP_SHELL_WHEEL_SCROLL_STATE
        .with(|slot| slot.borrow().as_ref().map(ScrollState::value_non_reactive))
        .expect("wheel scroll probe should expose its scroll state");
    let scrolled_bottom_y = find_layout_box_with_text(
        shell.layout_tree().expect("scrolled layout tree").root(),
        "Wheel scroll probe bottom",
    )
    .expect("bottom text in scrolled layout tree")
    .rect
    .y;

    assert!(
        scroll_offset > 0.0,
        "wheel event should change the scroll state before checking layout"
    );
    assert!(
        scrolled_bottom_y < initial_bottom_y - scroll_offset * 0.5,
        "scroll layout tree did not move with scroll offset: initial_y={initial_bottom_y} scrolled_y={scrolled_bottom_y} scroll_offset={scroll_offset}"
    );
}

#[test]
fn consumed_child_drag_does_not_scroll_parent_vertical_scroll() {
    let _guard = test_guard();
    APP_SHELL_WHEEL_SCROLL_STATE.with(|slot| slot.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_consumed_child_drag_scroll_probe,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    shell.update();

    let scroll_state = APP_SHELL_WHEEL_SCROLL_STATE
        .with(|slot| slot.borrow().as_ref().cloned())
        .expect("drag scroll probe should expose its scroll state");
    assert_eq!(scroll_state.value_non_reactive(), 0.0);

    assert!(shell.set_cursor(32.0, 32.0), "child should be hit-testable");
    assert!(shell.pointer_pressed(), "child should receive pointer down");
    assert!(
        shell.set_cursor(32.0, 96.0),
        "child drag should be delivered"
    );
    assert!(
        shell.pointer_released(),
        "captured child should receive pointer up"
    );
    shell.update();

    assert_eq!(
        scroll_state.value_non_reactive(),
        0.0,
        "parent vertical_scroll must ignore a drag consumed by a child pointer handler"
    );
}

#[test]
fn android_style_consecutive_flings_keep_direction() {
    let _guard = test_guard();
    APP_SHELL_WHEEL_SCROLL_STATE.with(|slot| slot.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_tall_fling_scroll_probe,
    );
    shell.set_buffer_size(320, 640);
    shell.set_viewport(320.0, 640.0);
    let mut frame_ns = 16_000_000u64;
    shell.update_at_frame_time_nanos(frame_ns);

    let scroll_state = APP_SHELL_WHEEL_SCROLL_STATE
        .with(|slot| slot.borrow().as_ref().cloned())
        .expect("fling probe should expose its scroll state");
    assert!(
        scroll_state.max_value() > 10_000.0,
        "probe content must leave plenty of scroll room"
    );

    let mut event_ms = 5_000_000i64;
    let mut velocities = Vec::new();

    for gesture in 0..3 {
        shell.set_cursor_at_time(160.0, 600.0, Some(event_ms));
        shell.pointer_pressed_at_time(Some(event_ms));

        let mut y = 600.0f32;
        for _ in 0..12 {
            event_ms += 8;
            y -= 8.0;
            shell.set_cursor_at_time(160.0, y, Some(event_ms));
        }

        event_ms += 8;
        shell.pointer_released_at_position_time(160.0, y + 4.0, Some(event_ms));

        velocities.push(shell.debug_enter_app_context(cranpose_ui::debug_last_fling_velocity));

        let offset_at_release = scroll_state.value_non_reactive();

        for _ in 0..19 {
            frame_ns += 16_000_000;
            shell.update_at_frame_time_nanos(frame_ns);
        }
        event_ms += 304 - 104;

        let offset_after_fling_window = scroll_state.value_non_reactive();
        assert!(
            offset_after_fling_window > offset_at_release + 20.0,
            "gesture {gesture}: fling must keep scrolling in the finger's travel direction \
             (offset_at_release={offset_at_release}, after={offset_after_fling_window}, \
             velocity={})",
            velocities[gesture],
        );
    }

    assert!(
        velocities.iter().all(|v| *v < 0.0),
        "all three identical upward flicks must compute the same (negative) velocity sign, \
         got {velocities:?}"
    );
}

#[test]
fn two_finger_pinch_reaches_zoomable_state_through_shell() {
    let _guard = test_guard();
    APP_SHELL_ZOOM_STATE.with(|slot| slot.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_zoomable_probe,
    );
    shell.set_buffer_size(320, 640);
    shell.set_viewport(320.0, 640.0);
    shell.update();

    let zoom_state = APP_SHELL_ZOOM_STATE
        .with(|slot| slot.borrow().as_ref().cloned())
        .expect("zoomable probe should expose its zoom state");
    assert_eq!(zoom_state.scale_non_reactive(), 1.0);

    let mut event_ms = 9_000_000i64;

    shell.set_cursor_at_time(140.0, 300.0, Some(event_ms));
    shell.pointer_pressed_at_time(Some(event_ms));

    event_ms += 16;
    assert!(
        shell.secondary_pointer_pressed(2, 220.0, 300.0, Some(event_ms)),
        "secondary pointer must reach the primary gesture's hit path"
    );

    for step in 1..=5i64 {
        event_ms += 8;
        shell.secondary_pointer_moved(2, 220.0 + step as f32 * 16.0, 300.0, Some(event_ms));
    }

    event_ms += 8;
    shell.secondary_pointer_released(2, 300.0, 300.0, Some(event_ms));
    shell.pointer_released_at_position_time(140.0, 300.0, Some(event_ms));
    shell.update();

    let scale = zoom_state.scale_non_reactive();
    assert!(
        (scale - 2.0).abs() < 0.05,
        "doubling the finger spread must double the zoom scale, got {scale}"
    );
}

#[test]
fn pointer_zoomed_reaches_zoomable_state() {
    let _guard = test_guard();
    APP_SHELL_ZOOM_STATE.with(|slot| slot.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_zoomable_probe,
    );
    shell.set_buffer_size(320, 640);
    shell.set_viewport(320.0, 640.0);
    shell.update();

    let zoom_state = APP_SHELL_ZOOM_STATE
        .with(|slot| slot.borrow().as_ref().cloned())
        .expect("zoomable probe should expose its zoom state");

    assert!(shell.set_cursor(160.0, 320.0), "probe should be hoverable");
    assert!(
        shell.pointer_zoomed(1.5),
        "zoom step must be consumed by the zoomable modifier"
    );
    shell.update();

    let scale = zoom_state.scale_non_reactive();
    assert!(
        (scale - 1.5).abs() < 1e-4,
        "ctrl+wheel zoom step must multiply the scale, got {scale}"
    );
}

#[test]
fn pointer_scrolled_reaches_horizontal_scroll_under_clickable_child() {
    let _guard = test_guard();
    APP_SHELL_WHEEL_SCROLL_STATE.with(|slot| slot.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_horizontal_clickable_wheel_scroll_probe,
    );
    shell.set_buffer_size(220, 120);
    shell.set_viewport(220.0, 120.0);
    shell.update();

    let scroll_state = APP_SHELL_WHEEL_SCROLL_STATE
        .with(|slot| slot.borrow().as_ref().cloned())
        .expect("horizontal wheel probe should expose its scroll state");
    assert_eq!(scroll_state.value_non_reactive(), 0.0);

    assert!(
        shell.set_cursor(72.0, 32.0),
        "clickable child in horizontal scroll row should be hit-testable"
    );
    assert!(
        shell.pointer_scrolled(-120.0, 0.0),
        "horizontal wheel event should be consumed by the parent scroll modifier"
    );
    shell.update();

    assert!(
        scroll_state.value_non_reactive() > 0.0,
        "horizontal wheel over a clickable child did not advance scroll state"
    );
}

#[test]
fn pointer_scrolled_reaches_real_lazy_column_modifier() {
    let _guard = test_guard();
    APP_SHELL_LAZY_LIST_STATE.with(|slot| slot.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        AppShellScrollIndicatorLazyList,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    shell.update();

    let list_state = APP_SHELL_LAZY_LIST_STATE
        .with(|slot| *slot.borrow())
        .expect("lazy wheel probe should expose its list state");
    assert_eq!(list_state.first_visible_item_index_non_reactive(), 0);
    assert_eq!(
        list_state.first_visible_item_scroll_offset_non_reactive(),
        0.0
    );

    assert!(
        shell.set_cursor(80.0, 120.0),
        "lazy list should be hit-testable"
    );
    assert!(
        shell.pointer_scrolled(0.0, -120.0),
        "wheel event should be consumed by the lazy scroll modifier"
    );
    shell.update();

    let moved_index = list_state.first_visible_item_index_non_reactive() > 0;
    let moved_offset = list_state.first_visible_item_scroll_offset_non_reactive() > 0.0;
    assert!(
        moved_index || moved_offset,
        "wheel event did not update LazyListState"
    );
    assert!(
        !shell.needs_redraw(),
        "lazy wheel scroll must not leave a redraw tail after the frame that applied the scroll"
    );
}

#[test]
fn pointer_drag_reaches_real_lazy_column_modifier() {
    let _guard = test_guard();
    APP_SHELL_LAZY_LIST_STATE.with(|slot| slot.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        AppShellScrollIndicatorLazyList,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    shell.update();

    let list_state = APP_SHELL_LAZY_LIST_STATE
        .with(|slot| *slot.borrow())
        .expect("lazy drag probe should expose its list state");
    assert_eq!(list_state.first_visible_item_index_non_reactive(), 0);
    assert_eq!(
        list_state.first_visible_item_scroll_offset_non_reactive(),
        0.0
    );

    assert!(
        shell.set_cursor(80.0, 180.0),
        "lazy list should be hit-testable before drag"
    );
    assert!(
        shell.pointer_pressed(),
        "lazy list should receive pointer down"
    );
    assert!(
        shell.set_cursor(80.0, 130.0),
        "first held move should stay on the captured lazy list path"
    );
    assert!(
        shell.set_cursor(80.0, 70.0),
        "second held move should scroll through the captured lazy list path"
    );
    shell.update();
    assert!(
        shell.pointer_released(),
        "lazy list should receive pointer release on the captured path"
    );
    shell.update();

    let moved_index = list_state.first_visible_item_index_non_reactive() > 0;
    let moved_offset = list_state.first_visible_item_scroll_offset_non_reactive() > 0.0;
    assert!(
        moved_index || moved_offset,
        "held pointer drag did not update LazyListState"
    );
    let layout_tree = shell
        .layout_tree()
        .expect("layout tree should be available after lazy drag");
    let layout_texts = layout_tree_texts(layout_tree);
    if moved_index {
        assert!(
            !layout_texts.iter().any(|text| text == "Row 0"),
            "retained lazy layout did not render the shifted item window after user drag: {layout_texts:?}"
        );
    }
}

#[test]
fn lazy_column_scroll_observer_recomposition_settles_to_idle() {
    let _guard = test_guard();
    APP_SHELL_LAZY_LIST_STATE.with(|slot| slot.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        AppShellScrollIndicatorLazyList,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    shell.update();

    assert!(shell.set_cursor(80.0, 120.0));
    assert!(shell.pointer_scrolled(0.0, -120.0));
    shell.update();

    let mut final_schedule = shell.frame_schedule();
    for _ in 0..8 {
        if !final_schedule.needs_update && !final_schedule.needs_frame {
            break;
        }
        shell.update();
        final_schedule = shell.frame_schedule();
    }

    assert!(
        !final_schedule.needs_update && !final_schedule.needs_frame,
        "lazy scroll observer invalidation did not settle: needs_update={} needs_frame={}",
        final_schedule.needs_update,
        final_schedule.needs_frame,
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
        let width_state = rememberMutableStateOf(|| 24.0f32);
        *state_holder_for_app.borrow_mut() = Some(width_state);
        draw_width_app(width_state);
    });

    shell.update();
    assert!(
        shell.layout_tree().is_some(),
        "layout tree should be available when a caller requests a snapshot"
    );
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

    let app_context = Rc::clone(&shell.app_context);
    app_context.enter(|| {
        shell
            .composition
            .process_invalid_scopes()
            .expect("recompose after width change");
    });
    shell.run_render_phase();
    assert!(
        shell.layout_tree.is_some(),
        "draw-only refresh should keep the retained layout tree available"
    );

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
fn draw_state_reads_schedule_draw_repass_without_composition_read() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let state_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
        Rc::new(RefCell::new(None));
    let state_holder_for_app = Rc::clone(&state_holder);

    let mut shell = AppShell::new(RecordingRenderer::default(), root_key, move || {
        let width_state = rememberMutableStateOf(|| 24.0f32);
        *state_holder_for_app.borrow_mut() = Some(width_state);
        draw_observed_width_app(width_state);
    });

    shell.update();
    let initial_scene = shell
        .renderer
        .last_scene
        .as_ref()
        .expect("expected initial render scene");
    let initial_width = find_rect_width(initial_scene, Color(0.2, 0.7, 0.3, 1.0))
        .expect("expected initial observed draw rect");

    let width_state = state_holder
        .borrow()
        .as_ref()
        .cloned()
        .expect("width state should be captured");
    width_state.set(120.0);

    shell.update();

    let updated_scene = shell
        .renderer
        .last_scene
        .as_ref()
        .expect("expected updated render scene");
    let updated_width = find_rect_width(updated_scene, Color(0.2, 0.7, 0.3, 1.0))
        .expect("expected updated observed draw rect");

    assert_ne!(
        initial_width, updated_width,
        "state reads inside draw closures must invalidate draw output"
    );
    assert!(
        (updated_width - 120.0).abs() < 0.1,
        "updated draw width should reflect latest state-only read"
    );
}

#[test]
fn draw_only_repass_uses_scoped_renderer_update() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let state_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
        Rc::new(RefCell::new(None));
    let state_holder_for_app = Rc::clone(&state_holder);
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let visual_updates = Rc::new(Cell::new(0));
    let last_dirty_nodes = Rc::new(RefCell::new(Vec::new()));

    let mut shell = AppShell::new(
        ScopedUpdateCountingRenderer::with_visual_updates(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::clone(&visual_updates),
            Rc::clone(&last_dirty_nodes),
        ),
        root_key,
        move || {
            let width_state = rememberMutableStateOf(|| 24.0f32);
            *state_holder_for_app.borrow_mut() = Some(width_state);
            draw_observed_width_app(width_state);
        },
    );

    shell.update();
    rebuilds.set(0);
    updates.set(0);
    visual_updates.set(0);
    last_dirty_nodes.borrow_mut().clear();

    let width_state = state_holder
        .borrow()
        .as_ref()
        .cloned()
        .expect("width state should be captured");
    width_state.set(120.0);

    shell.update();

    assert_eq!(
        updates.get(),
        0,
        "draw-only repass should not refresh hit data"
    );
    assert_eq!(
        visual_updates.get(),
        1,
        "draw-only repass should call the visual scoped renderer update"
    );
    assert_eq!(
        rebuilds.get(),
        0,
        "draw-only repass should not rebuild the full scene"
    );
    assert!(
        !last_dirty_nodes.borrow().is_empty(),
        "scoped renderer update should receive dirty node ids"
    );
}

#[test]
fn draw_only_scene_dirty_repass_uses_visual_scoped_renderer_update() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let state_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
        Rc::new(RefCell::new(None));
    let state_holder_for_app = Rc::clone(&state_holder);
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let visual_updates = Rc::new(Cell::new(0));
    let last_dirty_nodes = Rc::new(RefCell::new(Vec::new()));

    let mut shell = AppShell::new(
        ScopedUpdateCountingRenderer::with_visual_updates(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::clone(&visual_updates),
            Rc::clone(&last_dirty_nodes),
        ),
        root_key,
        move || {
            let width_state = rememberMutableStateOf(|| 24.0f32);
            *state_holder_for_app.borrow_mut() = Some(width_state);
            draw_observed_width_app(width_state);
        },
    );

    shell.update();
    rebuilds.set(0);
    updates.set(0);
    visual_updates.set(0);
    last_dirty_nodes.borrow_mut().clear();

    let width_state = state_holder
        .borrow()
        .as_ref()
        .cloned()
        .expect("width state should be captured");
    width_state.set(120.0);
    shell.scene_dirty = true;

    shell.update();

    assert_eq!(
        updates.get(),
        0,
        "draw-only scene dirtiness should not refresh hit data"
    );
    assert_eq!(
        visual_updates.get(),
        1,
        "draw-only scene dirtiness should use the visual scoped renderer update"
    );
    assert_eq!(
        rebuilds.get(),
        0,
        "draw-only scene dirtiness should not rebuild the full scene"
    );
    assert!(
        !last_dirty_nodes.borrow().is_empty(),
        "visual scoped update should receive dirty node ids"
    );
}

const KEYED_PAGE_A_COLOR: Color = Color(0.9, 0.1, 0.5, 1.0);

#[composable]
fn keyed_page_switch_app(
    tab: cranpose_core::MutableState<i32>,
    width_state: cranpose_core::MutableState<f32>,
) {
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        draw_observed_width_app(width_state);
        cranpose_core::with_key(&tab.get(), || {
            if tab.get() == 0 {
                Column(Modifier::empty(), ColumnSpec::default(), || {
                    for index in 0..3 {
                        Box(
                            Modifier::empty()
                                .size(Size {
                                    width: 80.0,
                                    height: 40.0,
                                })
                                .draw_behind(move |scope| {
                                    let color = if index == 2 {
                                        KEYED_PAGE_A_COLOR
                                    } else {
                                        Color(0.3, 0.3, 0.3, 1.0)
                                    };
                                    scope.draw_rect(Brush::solid(color));
                                }),
                            BoxSpec::default(),
                            || {},
                        );
                    }
                });
            }
        });
    });
}

fn graph_layer_contains_rect_color(
    layer: &cranpose_render_common::graph::LayerNode,
    color: Color,
) -> bool {
    for child in &layer.children {
        match child {
            cranpose_render_common::graph::RenderNode::Primitive(entry) => {
                if let cranpose_render_common::graph::PrimitiveNode::Draw(draw) = &entry.node
                    && let DrawPrimitive::Rect { brush, .. } = &draw.primitive
                    && *brush == Brush::solid(color)
                {
                    return true;
                }
            }
            cranpose_render_common::graph::RenderNode::DrawRun(run) => {
                for primitive in run.primitives.iter() {
                    if let DrawPrimitive::Rect { brush, .. } = primitive
                        && *brush == Brush::solid(color)
                    {
                        return true;
                    }
                }
            }
            cranpose_render_common::graph::RenderNode::Layer(inner) => {
                if graph_layer_contains_rect_color(inner, color) {
                    return true;
                }
            }
        }
    }
    false
}

#[test]
fn keyed_subtree_swap_with_pending_scoped_repass_evicts_stale_layers() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let tab_holder: Rc<RefCell<Option<cranpose_core::MutableState<i32>>>> =
        Rc::new(RefCell::new(None));
    let tab_holder_for_app = Rc::clone(&tab_holder);
    let width_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
        Rc::new(RefCell::new(None));
    let width_holder_for_app = Rc::clone(&width_holder);
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let visual_updates = Rc::new(Cell::new(0));
    let last_dirty_nodes = Rc::new(RefCell::new(Vec::new()));

    let mut shell = AppShell::new(
        ScopedUpdateCountingRenderer::with_visual_updates(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::clone(&visual_updates),
            Rc::clone(&last_dirty_nodes),
        ),
        root_key,
        move || {
            let tab = rememberMutableStateOf(|| 0i32);
            let width_state = rememberMutableStateOf(|| 24.0f32);
            *tab_holder_for_app.borrow_mut() = Some(tab);
            *width_holder_for_app.borrow_mut() = Some(width_state);
            keyed_page_switch_app(tab, width_state);
        },
    );

    shell.update();
    assert!(
        graph_layer_contains_rect_color(
            &shell
                .renderer()
                .scene()
                .graph
                .as_ref()
                .expect("initial graph")
                .root,
            KEYED_PAGE_A_COLOR,
        ),
        "page A must be in the graph after the initial frame"
    );

    let width_state = width_holder
        .borrow()
        .as_ref()
        .cloned()
        .expect("width state should be captured");
    width_state.set(120.0);
    shell.update();
    let sibling_node = *last_dirty_nodes
        .borrow()
        .first()
        .expect("draw repass should report the sibling node id");

    rebuilds.set(0);
    updates.set(0);
    visual_updates.set(0);
    last_dirty_nodes.borrow_mut().clear();

    let tab = tab_holder
        .borrow()
        .as_ref()
        .cloned()
        .expect("tab state should be captured");
    tab.set(1);
    let app_context = Rc::clone(shell.app_context());
    app_context.enter(|| cranpose_ui::schedule_layout_repass(sibling_node));
    shell.update();

    assert_eq!(
        rebuilds.get(),
        0,
        "the swap frame must stay on the scoped update path (the structural \
         parent joins the dirty set instead of forcing a full rebuild)"
    );
    assert_eq!(updates.get(), 1, "the swap frame must run a scoped update");
    let graph = shell
        .renderer()
        .scene()
        .graph
        .as_ref()
        .expect("graph after keyed swap");
    assert!(
        !graph_layer_contains_rect_color(&graph.root, KEYED_PAGE_A_COLOR),
        "page A's layers must be evicted from the graph on the swap frame \
         (cross-tab ghost: stale layers kept compositing on the presented path)"
    );
}

#[test]
fn lazy_column_scroll_repass_uses_scoped_renderer_update_without_stale_rows() {
    let _guard = test_guard();
    APP_SHELL_LAZY_LIST_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let visual_updates = Rc::new(Cell::new(0));
    let last_dirty_nodes = Rc::new(RefCell::new(Vec::new()));

    let mut shell = AppShell::new(
        ScopedUpdateCountingRenderer::with_visual_updates(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::clone(&visual_updates),
            Rc::clone(&last_dirty_nodes),
        ),
        root_key,
        AppShellScrollIndicatorLazyList,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    shell.update();

    let list_state = APP_SHELL_LAZY_LIST_STATE
        .with(|slot| *slot.borrow())
        .expect("lazy scroll probe should expose its list state");
    let initial_labels = graph_scene_text_values(shell.renderer.scene());
    let initial_rows = row_label_indices(&initial_labels);
    assert!(
        initial_rows.contains(&0),
        "initial graph should contain row zero before scrolling: {initial_labels:?}"
    );

    rebuilds.set(0);
    updates.set(0);
    visual_updates.set(0);
    last_dirty_nodes.borrow_mut().clear();

    assert!(
        list_state.dispatch_scroll_delta(-120.0).abs() > 0.0,
        "lazy scroll state should consume the programmatic scroll delta"
    );
    shell.update();

    assert_eq!(
        visual_updates.get(),
        0,
        "lazy-list layout repasses must refresh hit data"
    );
    assert_eq!(
        updates.get() + rebuilds.get(),
        1,
        "lazy-list layout repasses should perform exactly one renderer scene refresh"
    );
    if updates.get() > 0 {
        assert!(
            !last_dirty_nodes.borrow().is_empty(),
            "scoped lazy-list update should pass dirty layout node ids"
        );
    }

    let first_visible = list_state.first_visible_item_index_non_reactive();
    let updated_labels = graph_scene_text_values(shell.renderer.scene());
    let updated_rows = row_label_indices(&updated_labels);
    assert!(
        first_visible > 0,
        "test scroll should move the retained lazy-list first visible index"
    );
    assert!(
        !updated_rows.contains(&0),
        "partial lazy-list update must not retain stale row zero after scrolling: labels={updated_labels:?}"
    );
    assert!(
        updated_rows
            .windows(2)
            .all(|window| window[1] == window[0] + 1),
        "partial lazy-list update should keep a consecutive visible row window: rows={updated_rows:?}, labels={updated_labels:?}"
    );
    assert!(
        !shell.needs_redraw(),
        "lazy-list scroll must not leave a redraw tail after the applied frame"
    );
}

#[test]
fn lazy_column_scroll_never_rebuilds_the_whole_scene() {
    let _guard = test_guard();
    APP_SHELL_LAZY_LIST_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let visual_updates = Rc::new(Cell::new(0));
    let last_dirty_nodes = Rc::new(RefCell::new(Vec::new()));

    let mut shell = AppShell::new(
        ScopedUpdateCountingRenderer::with_visual_updates(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::clone(&visual_updates),
            Rc::clone(&last_dirty_nodes),
        ),
        root_key,
        AppShellScrollIndicatorLazyList,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    shell.update();

    let list_state = APP_SHELL_LAZY_LIST_STATE
        .with(|slot| *slot.borrow())
        .expect("lazy scroll probe should expose its list state");

    for step in 0..3 {
        rebuilds.set(0);
        updates.set(0);
        last_dirty_nodes.borrow_mut().clear();

        assert!(
            list_state.dispatch_scroll_delta(-40.0).abs() > 0.0,
            "lazy scroll state should consume scroll delta on step {step}"
        );
        shell.update();

        assert_eq!(
            rebuilds.get(),
            0,
            "lazy-list scroll step {step} must not rebuild the scene from the root"
        );
        assert_eq!(
            updates.get(),
            1,
            "lazy-list scroll step {step} should perform exactly one scoped scene update"
        );
        assert!(
            !last_dirty_nodes.borrow().is_empty(),
            "scoped lazy-list update on step {step} must carry the dirty node ids"
        );
    }
}

#[test]
fn graphics_layer_state_repass_does_not_recompose() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let state_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
        Rc::new(RefCell::new(None));
    let state_holder_for_app = Rc::clone(&state_holder);
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let visual_updates = Rc::new(Cell::new(0));
    let last_dirty_nodes = Rc::new(RefCell::new(Vec::new()));

    let mut shell = AppShell::new(
        ScopedUpdateCountingRenderer::with_visual_updates(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::clone(&visual_updates),
            Rc::clone(&last_dirty_nodes),
        ),
        root_key,
        move || {
            let offset_state = rememberMutableStateOf(|| 10.0f32);
            *state_holder_for_app.borrow_mut() = Some(offset_state);
            graphics_layer_observed_offset_app(offset_state);
        },
    );

    shell.update();
    let initial_recompositions = shell.fps_stats().recompositions;
    rebuilds.set(0);
    updates.set(0);
    visual_updates.set(0);
    last_dirty_nodes.borrow_mut().clear();

    let offset_state = state_holder
        .borrow()
        .as_ref()
        .cloned()
        .expect("offset state should be captured");
    offset_state.set(48.0);

    shell.update();

    assert_eq!(
        shell.fps_stats().recompositions,
        initial_recompositions,
        "graphics-layer-only state changes must not invalidate composition"
    );
    assert_eq!(
        updates.get(),
        0,
        "graphics-layer-only state changes should not use the hit-refresh update path"
    );
    assert_eq!(
        visual_updates.get(),
        1,
        "graphics-layer-only state changes should use visual scoped renderer update"
    );
    assert_eq!(
        rebuilds.get(),
        0,
        "graphics-layer-only state changes should not rebuild the full scene"
    );
    assert!(
        !last_dirty_nodes.borrow().is_empty(),
        "graphics-layer-only state changes should carry dirty node ids"
    );
}

#[test]
fn recomposed_graphics_layer_update_uses_scoped_renderer_update() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let state_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
        Rc::new(RefCell::new(None));
    let state_holder_for_app = Rc::clone(&state_holder);
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let last_dirty_nodes = Rc::new(RefCell::new(Vec::new()));

    let mut shell = AppShell::new(
        ScopedUpdateCountingRenderer::new(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::clone(&last_dirty_nodes),
        ),
        root_key,
        move || {
            let offset_state = rememberMutableStateOf(|| 10.0f32);
            *state_holder_for_app.borrow_mut() = Some(offset_state);
            graphics_layer_composed_offset_app(offset_state);
        },
    );

    shell.update();
    let initial_recompositions = shell.fps_stats().recompositions;
    rebuilds.set(0);
    updates.set(0);
    last_dirty_nodes.borrow_mut().clear();

    let offset_state = state_holder
        .borrow()
        .as_ref()
        .cloned()
        .expect("offset state should be captured");
    offset_state.set(48.0);

    shell.update();

    assert!(
        shell.fps_stats().recompositions > initial_recompositions,
        "composition-read graphics-layer state changes should still recompose the owning scope"
    );
    assert_eq!(
        updates.get(),
        1,
        "recomposed graphics-layer changes should use scoped renderer update"
    );
    assert_eq!(
        rebuilds.get(),
        0,
        "recomposed graphics-layer changes should not rebuild the full scene"
    );
    assert!(
        !last_dirty_nodes.borrow().is_empty(),
        "recomposed graphics-layer changes should carry dirty node ids"
    );
}

#[test]
fn recomposed_shader_effect_layers_use_scoped_renderer_update() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let time_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
        Rc::new(RefCell::new(None));
    let intensity_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
        Rc::new(RefCell::new(None));
    let time_holder_for_app = Rc::clone(&time_holder);
    let intensity_holder_for_app = Rc::clone(&intensity_holder);
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let visual_updates = Rc::new(Cell::new(0));
    let last_dirty_nodes = Rc::new(RefCell::new(Vec::new()));

    let mut shell = AppShell::new(
        ScopedUpdateCountingRenderer::with_visual_updates(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::clone(&visual_updates),
            Rc::clone(&last_dirty_nodes),
        ),
        root_key,
        move || {
            let time_state = rememberMutableStateOf(|| 0.0f32);
            let intensity_state = rememberMutableStateOf(|| 1.0f32);
            *time_holder_for_app.borrow_mut() = Some(time_state);
            *intensity_holder_for_app.borrow_mut() = Some(intensity_state);
            shader_rect_like_effect_layers_app(time_state, intensity_state);
        },
    );

    shell.update();
    let initial_recompositions = shell.fps_stats().recompositions;
    rebuilds.set(0);
    updates.set(0);
    visual_updates.set(0);
    last_dirty_nodes.borrow_mut().clear();

    let time_state = time_holder
        .borrow()
        .as_ref()
        .cloned()
        .expect("time state should be captured");
    let intensity_state = intensity_holder
        .borrow()
        .as_ref()
        .cloned()
        .expect("intensity state should be captured");
    time_state.set(0.25);
    intensity_state.set(1.5);

    shell.update();

    assert!(
        shell.fps_stats().recompositions > initial_recompositions,
        "shader-effect payload state changes should recompose the owning scope"
    );
    assert_eq!(
        updates.get(),
        1,
        "shader-effect payload changes should use one scoped renderer update"
    );
    assert_eq!(
        rebuilds.get(),
        0,
        "shader-effect payload changes should not rebuild the full scene"
    );
    assert!(
        last_dirty_nodes.borrow().len() >= 4,
        "all changed shader layer nodes should be delivered to the scoped renderer update"
    );
}

#[test]
fn lazy_shader_effect_layers_use_draw_repass_without_recomposition() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let time_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
        Rc::new(RefCell::new(None));
    let intensity_holder: Rc<RefCell<Option<cranpose_core::MutableState<f32>>>> =
        Rc::new(RefCell::new(None));
    let time_holder_for_app = Rc::clone(&time_holder);
    let intensity_holder_for_app = Rc::clone(&intensity_holder);
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let visual_updates = Rc::new(Cell::new(0));
    let last_dirty_nodes = Rc::new(RefCell::new(Vec::new()));

    let mut shell = AppShell::new(
        ScopedUpdateCountingRenderer::with_visual_updates(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::clone(&visual_updates),
            Rc::clone(&last_dirty_nodes),
        ),
        root_key,
        move || {
            let time_state = rememberMutableStateOf(|| 0.0f32);
            let intensity_state = rememberMutableStateOf(|| 1.0f32);
            *time_holder_for_app.borrow_mut() = Some(time_state);
            *intensity_holder_for_app.borrow_mut() = Some(intensity_state);
            shader_rect_like_lazy_effect_layers_app(time_state, intensity_state);
        },
    );

    shell.update();
    let initial_recompositions = shell.fps_stats().recompositions;
    rebuilds.set(0);
    updates.set(0);
    visual_updates.set(0);
    last_dirty_nodes.borrow_mut().clear();

    let time_state = time_holder
        .borrow()
        .as_ref()
        .cloned()
        .expect("time state should be captured");
    let intensity_state = intensity_holder
        .borrow()
        .as_ref()
        .cloned()
        .expect("intensity state should be captured");
    time_state.set(0.25);
    intensity_state.set(1.5);

    shell.update();

    assert_eq!(
        shell.fps_stats().recompositions,
        initial_recompositions,
        "lazy shader-effect payload state changes must not recompose"
    );
    assert_eq!(
        updates.get(),
        0,
        "lazy shader-effect payload changes should not use the hit-refresh update path"
    );
    assert_eq!(
        visual_updates.get(),
        1,
        "lazy shader-effect payload changes should use one visual scoped renderer update"
    );
    assert_eq!(
        rebuilds.get(),
        0,
        "lazy shader-effect payload changes should not rebuild the full scene"
    );
    assert!(
        last_dirty_nodes.borrow().len() >= 4,
        "all changed shader layer nodes should be delivered to the scoped renderer update"
    );
}

#[test]
fn graphics_layer_point_state_repass_does_not_recompose() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let state_holder: Rc<RefCell<Option<cranpose_core::MutableState<Point>>>> =
        Rc::new(RefCell::new(None));
    let state_holder_for_app = Rc::clone(&state_holder);
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let visual_updates = Rc::new(Cell::new(0));
    let last_dirty_nodes = Rc::new(RefCell::new(Vec::new()));

    let mut shell = AppShell::new(
        ScopedUpdateCountingRenderer::with_visual_updates(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::clone(&visual_updates),
            Rc::clone(&last_dirty_nodes),
        ),
        root_key,
        move || {
            let position_state = rememberMutableStateOf(|| Point { x: 10.0, y: 20.0 });
            *state_holder_for_app.borrow_mut() = Some(position_state);
            graphics_layer_observed_point_app(position_state);
        },
    );

    shell.update();
    let initial_recompositions = shell.fps_stats().recompositions;
    rebuilds.set(0);
    updates.set(0);
    visual_updates.set(0);
    last_dirty_nodes.borrow_mut().clear();

    let position_state = state_holder
        .borrow()
        .as_ref()
        .cloned()
        .expect("position state should be captured");
    position_state.set(Point { x: 48.0, y: 64.0 });

    shell.update();

    assert_eq!(
        shell.fps_stats().recompositions,
        initial_recompositions,
        "graphics-layer point state changes must not invalidate composition"
    );
    assert_eq!(
        updates.get(),
        0,
        "graphics-layer point state changes should not use the hit-refresh update path"
    );
    assert_eq!(
        visual_updates.get(),
        1,
        "graphics-layer point state changes should use visual scoped renderer update"
    );
    assert_eq!(
        rebuilds.get(),
        0,
        "graphics-layer point state changes should not rebuild the full scene"
    );
    assert!(
        !last_dirty_nodes.borrow().is_empty(),
        "scoped renderer update should receive the graphics-layer node id"
    );
}

#[test]
fn pointer_driven_graphics_layer_point_state_does_not_recompose() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let state_holder: Rc<RefCell<Option<cranpose_core::MutableState<Point>>>> =
        Rc::new(RefCell::new(None));
    let state_holder_for_app = Rc::clone(&state_holder);

    let mut shell = AppShell::new(HitGraphRenderer::default(), root_key, move || {
        let position_state = rememberMutableStateOf(Point::default);
        *state_holder_for_app.borrow_mut() = Some(position_state);
        pointer_driven_graphics_layer_point_app(position_state);
    });

    shell.update();
    let initial_recompositions = shell.fps_stats().recompositions;

    assert!(
        shell.set_cursor(20.0, 20.0),
        "initial hover should hit the draggable"
    );
    assert!(
        shell.pointer_pressed(),
        "pointer down should hit the draggable"
    );
    shell.update();

    for point in [
        Point { x: 28.0, y: 26.0 },
        Point { x: 36.0, y: 32.0 },
        Point { x: 44.0, y: 38.0 },
    ] {
        assert!(
            shell.set_cursor(point.x, point.y),
            "drag move should dispatch through the captured hit path"
        );
        shell.update();
    }

    assert!(
        shell.pointer_released(),
        "pointer release should dispatch through the captured hit path"
    );
    shell.update();

    assert_eq!(
        shell.fps_stats().recompositions,
        initial_recompositions,
        "pointer-driven graphics-layer state changes must not invalidate composition"
    );
}

#[test]
fn active_pointer_gesture_keeps_frame_schedule_until_release() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(HitGraphRenderer::default(), root_key, || {
        let position_state = rememberMutableStateOf(Point::default);
        pointer_driven_graphics_layer_point_app(position_state);
    });

    shell.update();
    assert!(!shell.frame_schedule().needs_frame);

    assert!(shell.set_cursor(20.0, 20.0));
    assert!(shell.pointer_pressed());
    assert!(shell.has_active_pointer_gesture());
    assert!(shell.frame_schedule().needs_frame);

    shell.update();
    assert!(
        shell.frame_schedule().needs_frame,
        "an active pointer gesture should keep the frame driver awake after the dirty frame is consumed"
    );

    assert!(shell.set_cursor(44.0, 38.0));
    shell.update();
    assert!(shell.frame_schedule().needs_frame);

    assert!(shell.pointer_released());
    shell.update();
    assert!(!shell.has_active_pointer_gesture());
    assert!(!shell.frame_schedule().needs_frame);
}

#[composable]
#[allow(non_snake_case)]
fn AbsoluteOffsetStackedTextRows(start: MutableState<i32>) {
    Box(
        Modifier::empty()
            .size_points(260.0, 220.0)
            .background(Color(0.02, 0.03, 0.05, 1.0)),
        BoxSpec::default(),
        move || {
            for row in 0..14 {
                Text(
                    format!("Row {:02}", start.get() + row),
                    Modifier::empty()
                        .size_points(220.0, 14.0)
                        .absolute_offset(12.0, 10.0 + row as f32 * 14.0),
                    TextStyle::default(),
                );
            }
        },
    );
}

fn rendered_text_values(scene: &cranpose_ui::RecordedRenderScene) -> Vec<String> {
    scene
        .operations()
        .iter()
        .filter_map(|op| match op {
            RenderOp::Text { value, .. } => Some(value.clone()),
            _ => None,
        })
        .collect()
}

thread_local! {
    static APP_SHELL_EXPANSION_STATE: RefCell<Option<MutableState<bool>>> =
        const { RefCell::new(None) };
}

#[composable]
#[allow(non_snake_case)]
fn AppShellGrowingFirstRow() {
    let expanded = rememberMutableStateOf(|| false);
    APP_SHELL_EXPANSION_STATE.with(|slot| *slot.borrow_mut() = Some(expanded));
    let list_state = rememberLazyListState();
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list_state,
        LazyColumnSpec::default(),
        |scope| {
            scope.items(3, move |index| {
                Column(Modifier::empty(), ColumnSpec::default(), move || {
                    if index == 0 {
                        Text(
                            "grower".to_string(),
                            Modifier::empty().height(24.0),
                            TextStyle::default(),
                        );
                        if expanded.get() {
                            Text(
                                "strip".to_string(),
                                Modifier::empty().height(40.0),
                                TextStyle::default(),
                            );
                        }
                    } else if index == 1 {
                        Text(
                            "sibling".to_string(),
                            Modifier::empty().height(24.0),
                            TextStyle::default(),
                        );
                    } else {
                        Text(
                            "tail".to_string(),
                            Modifier::empty().height(24.0),
                            TextStyle::default(),
                        );
                    }
                });
            });
        },
    );
}

thread_local! {
    static APP_SHELL_GROWER_STATE: RefCell<Option<MutableState<bool>>> =
        const { RefCell::new(None) };
}

#[composable]
#[allow(non_snake_case)]
fn AppShellGrowerBox() {
    let grown = rememberMutableStateOf(|| false);
    APP_SHELL_GROWER_STATE.with(|slot| *slot.borrow_mut() = Some(grown));
    let height = if grown.get() { 64.0 } else { 24.0 };
    Text(
        "grower".to_string(),
        Modifier::empty().height(height),
        TextStyle::default(),
    );
}

const ORDINARY_SIBLING_COLOR: cranpose_ui::Color = cranpose_ui::Color(0.9, 0.05, 0.55, 1.0);

#[composable]
#[allow(non_snake_case)]
fn AppShellOrdinaryColumnSiblings() {
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            AppShellGrowerBox();
            cranpose_ui::Box(
                Modifier::empty()
                    .size(cranpose_ui::Size::new(120.0, 24.0))
                    .background(ORDINARY_SIBLING_COLOR),
                cranpose_ui::BoxSpec::default(),
                || {},
            );
        },
    );
}

#[composable]
#[allow(non_snake_case)]
fn AppShellSizeReactiveTopology() {
    let size = rememberMutableStateOf(cranpose_ui::Size::default);
    cranpose_ui::Box(
        Modifier::empty().fill_max_size().report_size_state(size),
        cranpose_ui::BoxSpec::default(),
        move || {
            if size.get().width < 700.0 {
                Text(
                    "compact".to_string(),
                    Modifier::empty(),
                    TextStyle::default(),
                );
            } else {
                Text("wide".to_string(), Modifier::empty(), TextStyle::default());
            }
        },
    );
}

#[test]
fn size_reactive_topology_switches_on_resize_and_settles() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let last_dirty_nodes = Rc::new(RefCell::new(Vec::new()));

    let mut shell = AppShell::new(
        ScopedUpdateCountingRenderer::new(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::clone(&last_dirty_nodes),
        ),
        root_key,
        AppShellSizeReactiveTopology,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    for _ in 0..5 {
        shell.update();
        if !shell.needs_redraw() {
            break;
        }
    }
    let labels = graph_scene_text_values(shell.renderer.scene());
    assert!(
        labels.iter().any(|l| l == "compact") && !labels.iter().any(|l| l == "wide"),
        "at 320dp the size-reactive topology must be compact: {labels:?}"
    );

    shell.set_buffer_size(900, 600);
    shell.set_viewport(900.0, 600.0);
    let mut settle_frames = 0;
    for _ in 0..5 {
        shell.update();
        settle_frames += 1;
        if !shell.needs_redraw() && !shell.composition.should_recompose() {
            break;
        }
    }
    assert!(
        !shell.needs_redraw() && !shell.composition.should_recompose(),
        "a single resize must settle within 5 frames; a pending recomposition \
         here is the unconditional-write feedback loop (a size write during \
         measure that is not equality-gated recomposes forever)"
    );
    let labels = graph_scene_text_values(shell.renderer.scene());
    assert!(
        labels.iter().any(|l| l == "wide") && !labels.iter().any(|l| l == "compact"),
        "crossing 700dp must switch the composed topology to wide within \
         {settle_frames} settle frames: {labels:?}"
    );
}

#[composable]
#[allow(non_snake_case)]
fn AppShellSelfReferentialSize() {
    let size = rememberMutableStateOf(cranpose_ui::Size::default);
    let height = if size.get().height < 100.0 {
        200.0
    } else {
        50.0
    };
    cranpose_ui::Box(
        Modifier::empty().fill_max_width().report_size_state(size),
        cranpose_ui::BoxSpec::default(),
        move || {
            cranpose_ui::Box(
                Modifier::empty().size(cranpose_ui::Size::new(80.0, height)),
                cranpose_ui::BoxSpec::default(),
                || {},
            );
        },
    );
}

#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the oscillation ceiling that converts this livelock into a panic only exists in debug builds"
)]
#[should_panic(expected = "size-reactive feedback loop")]
fn a_self_referential_size_report_panics_in_debug_instead_of_livelocking() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let last_dirty_nodes = Rc::new(RefCell::new(Vec::new()));

    let mut shell = AppShell::new(
        ScopedUpdateCountingRenderer::new(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::clone(&last_dirty_nodes),
        ),
        root_key,
        AppShellSelfReferentialSize,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    for _ in 0..200 {
        shell.update();
    }
}

thread_local! {
    static CLEAN_SLOT_LIST_STATE: RefCell<Option<LazyListState>> = const { RefCell::new(None) };
    static CLEAN_SLOT_COMPOSE_COUNT: Cell<u32> = const { Cell::new(0) };
    static CLEAN_SLOT_MEASURE_COUNT: Cell<u32> = const { Cell::new(0) };
}

#[composable]
fn app_shell_clean_slot_scroll_probe() {
    cranpose_ui::SubcomposeLayout(
        Modifier::empty().fill_max_size(),
        move |scope, constraints| {
            CLEAN_SLOT_MEASURE_COUNT.with(|count| count.set(count.get() + 1));
            let width = constraints.max_width.max(constraints.min_width);
            let height = constraints.max_height.max(constraints.min_height);
            let nodes = scope.subcompose(cranpose_core::SlotId::new(0), (), move || {
                CLEAN_SLOT_COMPOSE_COUNT.with(|count| count.set(count.get() + 1));
                let list_state = rememberLazyListState();
                CLEAN_SLOT_LIST_STATE.with(|slot| {
                    *slot.borrow_mut() = Some(list_state);
                });
                LazyColumn(
                    Modifier::empty().fill_max_size(),
                    list_state,
                    LazyColumnSpec::new(),
                    |scope| {
                        scope.items(80, |index| {
                            Text(
                                format!("Clean slot row {index}"),
                                Modifier::empty().height(48.0),
                                TextStyle::default(),
                            );
                        });
                    },
                );
            });
            let mut placements = Vec::with_capacity(nodes.len());
            for node in nodes {
                let placeable = scope.measure(node, cranpose_ui::Constraints::tight(width, height));
                placements.push(cranpose_ui::Placement::new(
                    placeable.node_id(),
                    0.0,
                    0.0,
                    0,
                ));
            }
            scope.layout(width, height, placements)
        },
    );
}

#[test]
fn a_scroll_measure_repass_does_not_recompose_a_subcompose_slot_that_read_no_scrolled_state() {
    let _guard = test_guard();
    CLEAN_SLOT_LIST_STATE.with(|slot| slot.borrow_mut().take());
    CLEAN_SLOT_COMPOSE_COUNT.with(|count| count.set(0));
    CLEAN_SLOT_MEASURE_COUNT.with(|count| count.set(0));

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        TestRenderer::default(),
        root_key,
        app_shell_clean_slot_scroll_probe,
    );
    shell.set_buffer_size(320, 480);
    shell.set_viewport(320.0, 480.0);
    for _ in 0..5 {
        shell.update();
        if !shell.needs_redraw() && !shell.composition.should_recompose() {
            break;
        }
    }
    let settled_composes = CLEAN_SLOT_COMPOSE_COUNT.with(Cell::get);
    let settled_measures = CLEAN_SLOT_MEASURE_COUNT.with(Cell::get);
    let settled_skips = cranpose_ui::clean_slot_skip_count();
    assert!(
        settled_composes >= 1 && settled_measures >= 1,
        "instrument dead: the probe never composed ({settled_composes}) or \
         measured ({settled_measures}) its slot while settling"
    );

    let list_state = CLEAN_SLOT_LIST_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("probe must expose its lazy list state");

    for target in [4usize, 8, 12] {
        list_state.scroll_to_item(target, 0.0);
        for _ in 0..4 {
            shell.update();
            if !shell.needs_redraw() && !shell.has_active_animations() {
                break;
            }
        }
    }

    let measures_after = CLEAN_SLOT_MEASURE_COUNT.with(Cell::get);
    assert!(
        measures_after > settled_measures,
        "liveness: scrolling the list inside the slot must re-run the \
         subcompose measure policy (settled at {settled_measures}, still \
         {measures_after} after three scrolls) — if this fails the test no \
         longer exercises the measure repass at all"
    );
    let layout_texts = layout_tree_texts(shell.layout_tree().expect("layout tree"));
    assert!(
        layout_texts.iter().any(|text| text == "Clean slot row 12"),
        "liveness: the scroll must actually reach row 12 in the layout tree, \
         got {layout_texts:?}"
    );

    let skips_after = cranpose_ui::clean_slot_skip_count();
    assert!(
        skips_after >= settled_skips + (measures_after - settled_measures) as u64,
        "a scroll-driven measure repass recomposed a subcompose slot whose \
         composition read none of the scrolled state: only {} of the {} \
         extra measure passes reused the retained slot. Every non-reused \
         pass is the compose walk over the slot's retained composition \
         (measured at ~0.3ms per body-sized slot per pass on a Kirin 980, \
         and paid by whichever layer owns the slot — see \
         docs/subcompose_measure_cost.md). A measure pass over a slot with \
         no invalidated scopes must reuse the retained slot roots without \
         composing",
        skips_after - settled_skips,
        measures_after - settled_measures,
    );

    #[cfg(not(debug_assertions))]
    {
        let composes_after = CLEAN_SLOT_COMPOSE_COUNT.with(Cell::get);
        assert_eq!(
            composes_after, settled_composes,
            "the release skip path must not run the slot content closure at all"
        );
    }
}

thread_local! {
    static SLOT_STATE_HANDLE: RefCell<Option<MutableState<i32>>> = const { RefCell::new(None) };
}

#[composable]
fn app_shell_slot_reads_state_probe() {
    let value = rememberMutableStateOf(|| 1);
    SLOT_STATE_HANDLE.with(|slot| {
        *slot.borrow_mut() = Some(value);
    });
    cranpose_ui::SubcomposeLayout(
        Modifier::empty().fill_max_size(),
        move |scope, constraints| {
            let width = constraints.max_width.max(constraints.min_width);
            let height = constraints.max_height.max(constraints.min_height);
            let nodes = scope.subcompose(cranpose_core::SlotId::new(0), (), move || {
                Text(
                    format!("slot value {}", value.get()),
                    Modifier::empty(),
                    TextStyle::default(),
                );
            });
            let mut placements = Vec::with_capacity(nodes.len());
            for node in nodes {
                let placeable = scope.measure(node, cranpose_ui::Constraints::tight(width, height));
                placements.push(cranpose_ui::Placement::new(
                    placeable.node_id(),
                    0.0,
                    0.0,
                    0,
                ));
            }
            scope.layout(width, height, placements)
        },
    );
}

#[test]
fn a_state_write_reaches_slot_content_without_any_concurrent_layout_dirt() {
    let _guard = test_guard();
    SLOT_STATE_HANDLE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        TestRenderer::default(),
        root_key,
        app_shell_slot_reads_state_probe,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    for _ in 0..5 {
        shell.update();
        if !shell.needs_redraw() && !shell.composition.should_recompose() {
            break;
        }
    }
    let layout_texts = layout_tree_texts(shell.layout_tree().expect("layout tree"));
    assert!(
        layout_texts.iter().any(|text| text == "slot value 1"),
        "instrument dead: initial slot text missing, got {layout_texts:?}"
    );

    let value = SLOT_STATE_HANDLE
        .with(|slot| slot.borrow().as_ref().cloned())
        .expect("probe must expose its state");
    value.set(2);
    assert!(
        shell.composition.should_recompose(),
        "the load-bearing claim of slot retention: a write to state read only \
         inside a measure-time slot composition must invalidate that slot's \
         recompose scope BEFORE any measure pass runs. Slot reads ride \
         scope-level state subscriptions, not the per-pass measure observer \
         (which is constructed fresh, never started, and dies with the pass); \
         if this fails, nothing can carry 'still clean' across passes and \
         clean-slot reuse is unsound"
    );
    for _ in 0..5 {
        shell.update();
        if !shell.needs_redraw() && !shell.composition.should_recompose() {
            break;
        }
    }
    let layout_texts = layout_tree_texts(shell.layout_tree().expect("layout tree"));
    assert!(
        layout_texts.iter().any(|text| text == "slot value 2"),
        "a state write read only by subcompose slot content must reach the \
         screen without any concurrent layout dirt; the slot still shows \
         {layout_texts:?}. If this regresses, slot read-subscriptions are \
         dying with the per-pass measure observer and nothing else schedules \
         the remeasure"
    );
}

thread_local! {
    static UNKEYED_CELL: Cell<u32> = const { Cell::new(0) };
    static KEYED_HEIGHT_STATE: RefCell<Option<MutableState<f32>>> = const { RefCell::new(None) };
}

#[composable]
fn app_shell_unkeyed_cell_slot_probe() {
    cranpose_ui::SubcomposeLayout(
        Modifier::empty().fill_max_size(),
        move |scope, constraints| {
            let width = constraints.max_width.max(constraints.min_width);
            let height = constraints.max_height.max(constraints.min_height);
            let poison = UNKEYED_CELL.with(Cell::get);
            let mut placements = Vec::new();
            let nodes = scope.subcompose(cranpose_core::SlotId::new(0), (), move || {
                for row in 0..=poison.min(3) {
                    Text(
                        format!("poisoned row {row}"),
                        Modifier::empty().height(20.0),
                        TextStyle::default(),
                    );
                }
            });
            for node in nodes {
                let placeable = scope.measure(node, cranpose_ui::Constraints::loose(width, height));
                placements.push(cranpose_ui::Placement::new(
                    placeable.node_id(),
                    0.0,
                    0.0,
                    1,
                ));
            }
            let list_nodes = scope.subcompose(cranpose_core::SlotId::new(1), (), move || {
                let list_state = rememberLazyListState();
                CLEAN_SLOT_LIST_STATE.with(|slot| {
                    *slot.borrow_mut() = Some(list_state);
                });
                LazyColumn(
                    Modifier::empty().fill_max_size(),
                    list_state,
                    LazyColumnSpec::new(),
                    |scope| {
                        scope.items(80, |index| {
                            Text(
                                format!("driver row {index}"),
                                Modifier::empty().height(48.0),
                                TextStyle::default(),
                            );
                        });
                    },
                );
            });
            for node in list_nodes {
                let placeable = scope.measure(node, cranpose_ui::Constraints::tight(width, height));
                placements.push(cranpose_ui::Placement::new(
                    placeable.node_id(),
                    0.0,
                    0.0,
                    0,
                ));
            }
            scope.layout(width, height, placements)
        },
    );
}

#[test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "the shadow oracle that converts this staleness into a panic only exists in debug builds"
)]
#[should_panic(expected = "clean-slot skip diverged")]
fn a_slot_reading_a_changed_cell_outside_the_key_panics_in_debug_instead_of_going_stale() {
    let _guard = test_guard();
    UNKEYED_CELL.with(|cell| cell.set(0));
    CLEAN_SLOT_LIST_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        TestRenderer::default(),
        root_key,
        app_shell_unkeyed_cell_slot_probe,
    );
    shell.set_buffer_size(320, 480);
    shell.set_viewport(320.0, 480.0);
    for _ in 0..5 {
        shell.update();
        if !shell.needs_redraw() && !shell.composition.should_recompose() {
            break;
        }
    }
    UNKEYED_CELL.with(|cell| cell.set(2));
    let list_state = CLEAN_SLOT_LIST_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("probe must expose its lazy list state");
    list_state.scroll_to_item(6, 0.0);
    for _ in 0..4 {
        shell.update();
    }
}

#[composable]
fn app_shell_keyed_measure_input_probe() {
    let height = rememberMutableStateOf(|| 40.0f32);
    KEYED_HEIGHT_STATE.with(|slot| {
        *slot.borrow_mut() = Some(height);
    });
    cranpose_ui::SubcomposeLayout(
        Modifier::empty().fill_max_size(),
        move |scope, constraints| {
            let width = constraints.max_width.max(constraints.min_width);
            let total_height = constraints.max_height.max(constraints.min_height);
            let bar_nodes = scope.subcompose(cranpose_core::SlotId::new(0), (), move || {
                Box(
                    Modifier::empty().height(height.get()),
                    BoxSpec::default(),
                    || {},
                );
            });
            let mut bar_height = 0.0f32;
            let mut placements = Vec::new();
            for node in bar_nodes {
                let placeable =
                    scope.measure(node, cranpose_ui::Constraints::loose(width, total_height));
                bar_height = bar_height.max(placeable.height());
                placements.push(cranpose_ui::Placement::new(
                    placeable.node_id(),
                    0.0,
                    0.0,
                    1,
                ));
            }
            let padding = bar_height;
            let content_nodes =
                scope.subcompose(cranpose_core::SlotId::new(1), padding, move || {
                    Text(
                        format!("content top pad {padding}"),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                });
            for node in content_nodes {
                let placeable = scope.measure(
                    node,
                    cranpose_ui::Constraints::tight(width, total_height - padding),
                );
                placements.push(cranpose_ui::Placement::new(
                    placeable.node_id(),
                    0.0,
                    padding,
                    0,
                ));
            }
            scope.layout(width, total_height, placements)
        },
    );
}

#[test]
fn a_measure_computed_capture_reaches_slot_content_through_the_subcompose_key() {
    let _guard = test_guard();
    KEYED_HEIGHT_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        TestRenderer::default(),
        root_key,
        app_shell_keyed_measure_input_probe,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    for _ in 0..5 {
        shell.update();
        if !shell.needs_redraw() && !shell.composition.should_recompose() {
            break;
        }
    }
    let layout_texts = layout_tree_texts(shell.layout_tree().expect("layout tree"));
    assert!(
        layout_texts.iter().any(|text| text == "content top pad 40"),
        "instrument dead: initial keyed padding missing, got {layout_texts:?}"
    );

    let height = KEYED_HEIGHT_STATE
        .with(|slot| slot.borrow().as_ref().cloned())
        .expect("probe must expose its bar height state");
    height.set(72.0);
    for _ in 0..5 {
        shell.update();
        if !shell.needs_redraw() && !shell.composition.should_recompose() {
            break;
        }
    }
    let layout_texts = layout_tree_texts(shell.layout_tree().expect("layout tree"));
    assert!(
        layout_texts.iter().any(|text| text == "content top pad 72"),
        "a value computed inside the measure policy (a scaffold's padding) \
         never invalidates a recompose scope, so the subcompose KEY is its \
         only path into retained slot content; the slot still shows \
         {layout_texts:?}. If this fails, clean-slot reuse is skipping on a \
         stale capture key"
    );
}

thread_local! {
    static CAROUSEL_SCROLL_STATE: RefCell<Option<ScrollState>> = const { RefCell::new(None) };
}

#[composable]
fn app_shell_lazy_carousel_probe() {
    let list_state = rememberLazyListState();
    LazyColumn(
        Modifier::empty().fill_max_size(),
        list_state,
        LazyColumnSpec::new(),
        |scope| {
            scope.items(20, |index| {
                if index == 0 {
                    let inner =
                        cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
                    CAROUSEL_SCROLL_STATE.with(|slot| {
                        *slot.borrow_mut() = Some(inner);
                    });
                    Row(
                        Modifier::empty()
                            .fill_max_width()
                            .height(60.0)
                            .horizontal_scroll(inner, false),
                        RowSpec::new(),
                        || {
                            Text(
                                "carousel start",
                                Modifier::empty().width(160.0),
                                TextStyle::default(),
                            );
                            Spacer(Size {
                                width: 600.0,
                                height: 0.0,
                            });
                            Text(
                                "carousel end",
                                Modifier::empty().width(160.0),
                                TextStyle::default(),
                            );
                        },
                    );
                } else {
                    Text(
                        format!("plain row {index}"),
                        Modifier::empty().height(48.0),
                        TextStyle::default(),
                    );
                }
            });
        },
    );
}

#[test]
fn a_horizontal_scroll_inside_a_lazy_item_still_moves_when_its_measurement_is_cached() {
    let _guard = test_guard();
    CAROUSEL_SCROLL_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        TestRenderer::default(),
        root_key,
        app_shell_lazy_carousel_probe,
    );
    shell.set_buffer_size(320, 480);
    shell.set_viewport(320.0, 480.0);
    for _ in 0..5 {
        shell.update();
        if !shell.needs_redraw() && !shell.composition.should_recompose() {
            break;
        }
    }
    let initial_x = find_layout_box_with_text(
        shell.layout_tree().expect("layout tree").root(),
        "carousel start",
    )
    .expect("carousel start in layout tree")
    .rect
    .x;

    let inner = CAROUSEL_SCROLL_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("probe must expose the inner scroll state");
    shell.app_context().clone().enter(|| inner.scroll_to(80.0));
    for _ in 0..4 {
        shell.update();
        if !shell.needs_redraw() && !shell.composition.should_recompose() {
            break;
        }
    }
    let offset = inner.value_non_reactive();
    assert!(
        offset > 0.0,
        "instrument dead: the inner scroll state never moved ({offset})"
    );
    let scrolled_x = find_layout_box_with_text(
        shell.layout_tree().expect("layout tree").root(),
        "carousel start",
    )
    .expect("carousel start after scroll")
    .rect
    .x;
    assert!(
        scrolled_x < initial_x - 40.0,
        "a placement-only repass (schedule_layout_repass from a scroll offset) \
         was swallowed: the inner row still sits at x={scrolled_x} after \
         scrolling to offset {offset} (was x={initial_x}). The cached-measure \
         gate must reject a needs_layout child instead of serving the cached \
         measurement and clearing the flag"
    );
}

fn graph_scene_solid_rect_y(
    scene: &cranpose_render_common::graph_scene::Scene,
    color: cranpose_ui::Color,
) -> Option<f32> {
    fn brush_matches(brush: &cranpose_ui_graphics::Brush, color: cranpose_ui::Color) -> bool {
        matches!(brush, cranpose_ui_graphics::Brush::Solid(c) if (c.0 - color.0).abs() < 0.01
            && (c.1 - color.1).abs() < 0.01
            && (c.2 - color.2).abs() < 0.01)
    }

    fn walk(
        layer: &cranpose_render_common::graph::LayerNode,
        offset_y: f32,
        color: cranpose_ui::Color,
    ) -> Option<f32> {
        let base = offset_y
            + layer
                .transform_to_parent
                .map_point(cranpose_ui_graphics::Point { x: 0.0, y: 0.0 })
                .y
            + layer.content_offset.y;
        for child in &layer.children {
            match child {
                cranpose_render_common::graph::RenderNode::Layer(child_layer) => {
                    if let Some(y) = walk(child_layer, base, color) {
                        return Some(y);
                    }
                }
                cranpose_render_common::graph::RenderNode::Primitive(entry) => {
                    if let cranpose_render_common::graph::PrimitiveNode::Draw(draw) = &entry.node {
                        match &draw.primitive {
                            cranpose_ui_graphics::DrawPrimitive::Rect { rect, brush, .. }
                            | cranpose_ui_graphics::DrawPrimitive::RoundRect {
                                rect, brush, ..
                            } if brush_matches(brush, color) => {
                                return Some(base + rect.y);
                            }
                            _ => {}
                        }
                    }
                }
                cranpose_render_common::graph::RenderNode::DrawRun(run) => {
                    for primitive in run.primitives.iter() {
                        match primitive {
                            cranpose_ui_graphics::DrawPrimitive::Rect { rect, brush, .. }
                            | cranpose_ui_graphics::DrawPrimitive::RoundRect {
                                rect, brush, ..
                            } if brush_matches(brush, color) => {
                                return Some(base + rect.y);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        None
    }

    let graph = scene.graph.as_ref()?;
    walk(&graph.root, 0.0, color)
}

#[test]
fn sibling_in_an_ordinary_column_moves_in_the_scene_when_a_row_grows() {
    let _guard = test_guard();
    APP_SHELL_GROWER_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let last_dirty_nodes = Rc::new(RefCell::new(Vec::new()));

    let mut shell = AppShell::new(
        ScopedUpdateCountingRenderer::new(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::clone(&last_dirty_nodes),
        ),
        root_key,
        AppShellOrdinaryColumnSiblings,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    shell.update();

    let grown = APP_SHELL_GROWER_STATE
        .with(|slot| *slot.borrow())
        .expect("grower probe should expose its state");
    let resting_sibling_y =
        graph_scene_solid_rect_y(shell.renderer.scene(), ORDINARY_SIBLING_COLOR)
            .expect("resting scene should contain the sibling box");

    rebuilds.set(0);
    updates.set(0);

    grown.set_value(true);
    shell.update();

    let moved_sibling_y = graph_scene_solid_rect_y(shell.renderer.scene(), ORDINARY_SIBLING_COLOR)
        .expect("grown scene should still contain the sibling box");
    assert!(
        moved_sibling_y >= resting_sibling_y + 39.0,
        "an ordinary column's sibling must move in the SCENE when the row \
         above grows 24->64: resting y={resting_sibling_y}, after growth \
         y={moved_sibling_y}"
    );
    assert_eq!(
        rebuilds.get(),
        0,
        "the moved sibling must reach the SCOPED update; a whole-scene rebuild \
         hides the missing geometry channel and pays O(app) for one grown row"
    );
    assert!(
        updates.get() >= 1,
        "the growth frame must run at least one scoped update"
    );
}

#[composable]
#[allow(non_snake_case)]
fn AppShellGrowerAboveNestedLazy() {
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            AppShellGrowerBox();
            let nested_state = rememberLazyListState();
            LazyColumn(
                Modifier::empty().fill_max_width().height(120.0),
                nested_state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.items(3, move |index| {
                        Text(
                            format!("nested {index}"),
                            Modifier::empty().height(24.0),
                            TextStyle::default(),
                        );
                    });
                },
            );
        },
    );
}

#[test]
fn a_nested_lazy_list_moves_in_the_scene_when_the_row_above_grows() {
    let _guard = test_guard();
    APP_SHELL_GROWER_STATE.with(|slot| slot.borrow_mut().take());
    APP_SHELL_LAZY_LIST_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let last_dirty_nodes = Rc::new(RefCell::new(Vec::new()));

    let mut shell = AppShell::new(
        ScopedUpdateCountingRenderer::new(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::clone(&last_dirty_nodes),
        ),
        root_key,
        AppShellGrowerAboveNestedLazy,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    shell.update();

    let grown = APP_SHELL_GROWER_STATE
        .with(|slot| *slot.borrow())
        .expect("grower probe should expose its state");
    let resting_nested_y = graph_scene_text_y(shell.renderer.scene(), "nested 0")
        .expect("resting scene should contain the nested list's first row");

    rebuilds.set(0);
    updates.set(0);

    grown.set_value(true);
    shell.update();

    let moved_nested_y = graph_scene_text_y(shell.renderer.scene(), "nested 0")
        .expect("grown scene should still contain the nested list's first row");
    assert!(
        moved_nested_y >= resting_nested_y + 39.0,
        "a nested lazy list must move in the SCENE when the row above grows \
         24->64: resting y={resting_nested_y}, after growth y={moved_nested_y}"
    );
    assert_eq!(
        rebuilds.get(),
        0,
        "the moved nested list must reach the SCOPED update, not ride a \
         whole-scene rebuild"
    );
}

fn graph_scene_text_y(
    scene: &cranpose_render_common::graph_scene::Scene,
    needle: &str,
) -> Option<f32> {
    fn walk(
        layer: &cranpose_render_common::graph::LayerNode,
        offset_y: f32,
        needle: &str,
    ) -> Option<f32> {
        let base = offset_y
            + layer
                .transform_to_parent
                .map_point(cranpose_ui_graphics::Point { x: 0.0, y: 0.0 })
                .y
            + layer.content_offset.y;
        for child in &layer.children {
            match child {
                cranpose_render_common::graph::RenderNode::Layer(child_layer) => {
                    if let Some(y) = walk(child_layer, base, needle) {
                        return Some(y);
                    }
                }
                cranpose_render_common::graph::RenderNode::Primitive(entry) => {
                    if let cranpose_render_common::graph::PrimitiveNode::Text(text) = &entry.node
                        && text.text.text == needle
                    {
                        return Some(base + text.rect.y);
                    }
                }
                cranpose_render_common::graph::RenderNode::DrawRun(_) => {}
            }
        }
        None
    }

    let graph = scene.graph.as_ref()?;
    walk(&graph.root, 0.0, needle)
}

#[test]
fn sibling_moved_by_another_rows_growth_reaches_the_scoped_scene_update() {
    let _guard = test_guard();
    APP_SHELL_EXPANSION_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let rebuilds = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let last_dirty_nodes = Rc::new(RefCell::new(Vec::new()));

    let mut shell = AppShell::new(
        ScopedUpdateCountingRenderer::new(
            Rc::clone(&rebuilds),
            Rc::clone(&updates),
            Rc::clone(&last_dirty_nodes),
        ),
        root_key,
        AppShellGrowingFirstRow,
    );
    shell.set_buffer_size(320, 240);
    shell.set_viewport(320.0, 240.0);
    shell.update();

    let expanded = APP_SHELL_EXPANSION_STATE
        .with(|slot| *slot.borrow())
        .expect("expansion probe should expose its state");
    let resting_sibling_y = graph_scene_text_y(shell.renderer.scene(), "sibling")
        .expect("resting scene should contain the sibling row");

    rebuilds.set(0);
    updates.set(0);

    expanded.set_value(true);
    shell.update();

    let strip_y = graph_scene_text_y(shell.renderer.scene(), "strip")
        .expect("expanded scene should contain the strip");
    let moved_sibling_y = graph_scene_text_y(shell.renderer.scene(), "sibling")
        .expect("expanded scene should still contain the sibling row");
    assert!(
        moved_sibling_y > resting_sibling_y + 1.0,
        "the sibling below a grown row must move down in the SCENE, not only in \
         layout: resting y={resting_sibling_y}, after growth y={moved_sibling_y}, \
         strip y={strip_y}"
    );
    assert!(
        strip_y < moved_sibling_y,
        "the strip must sit above the pushed sibling: strip y={strip_y}, \
         sibling y={moved_sibling_y}"
    );
    assert_eq!(
        rebuilds.get(),
        0,
        "the moved sibling must reach the SCOPED update; a full rebuild hides \
         the missing geometry channel and pays O(whole app) for one grown row"
    );
}

fn graph_scene_text_values(scene: &cranpose_render_common::graph_scene::Scene) -> Vec<String> {
    fn collect(layer: &cranpose_render_common::graph::LayerNode, out: &mut Vec<String>) {
        for child in &layer.children {
            match child {
                cranpose_render_common::graph::RenderNode::Layer(child_layer) => {
                    collect(child_layer, out);
                }
                cranpose_render_common::graph::RenderNode::Primitive(entry) => {
                    if let cranpose_render_common::graph::PrimitiveNode::Text(text) = &entry.node {
                        out.push(text.text.text.clone());
                    }
                }
                cranpose_render_common::graph::RenderNode::DrawRun(_) => {}
            }
        }
    }

    let mut values = Vec::new();
    if let Some(graph) = &scene.graph {
        collect(&graph.root, &mut values);
    }
    values
}

fn row_label_indices(values: &[String]) -> Vec<usize> {
    values
        .iter()
        .filter_map(|value| {
            value
                .strip_prefix("Row ")
                .and_then(|index| index.parse::<usize>().ok())
        })
        .collect()
}

#[test]
fn absolute_offset_text_rows_redraw_after_state_only_change() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let state_holder: Rc<RefCell<Option<MutableState<i32>>>> = Rc::new(RefCell::new(None));
    let state_holder_for_app = Rc::clone(&state_holder);

    let mut shell = AppShell::new(RecordingRenderer::default(), root_key, move || {
        let start = rememberMutableStateOf(|| 0i32);
        *state_holder_for_app.borrow_mut() = Some(start);
        AbsoluteOffsetStackedTextRows(start);
    });

    shell.update();
    let initial_scene = shell
        .renderer
        .last_scene
        .as_ref()
        .expect("expected initial render scene");
    let initial_texts = rendered_text_values(initial_scene);
    assert!(initial_texts.iter().any(|text| text == "Row 00"));
    assert!(!initial_texts.iter().any(|text| text == "Row 30"));

    let start = state_holder
        .borrow()
        .as_ref()
        .cloned()
        .expect("start state should be captured");
    start.set(30);
    shell.update();

    let updated_scene = shell
        .renderer
        .last_scene
        .as_ref()
        .expect("expected updated render scene");
    let updated_texts = rendered_text_values(updated_scene);

    assert!(
        updated_texts.iter().any(|text| text == "Row 30"),
        "render scene should contain recomposed absolute-offset text rows, got {updated_texts:?}"
    );
    assert!(
        !updated_texts.iter().any(|text| text == "Row 00"),
        "render scene must not keep stale absolute-offset text rows, got {updated_texts:?}"
    );
}

#[test]
fn app_shell_new_drains_root_render_requests_before_first_frame() {
    let _guard = test_guard();
    ROOT_RENDER_TEST_INVALIDATED.with(|flag| flag.set(false));

    let root_key = location_key(file!(), line!(), column!());
    let render_count = Rc::new(Cell::new(0));
    let render_count_for_app = Rc::clone(&render_count);
    let mut shell = AppShell::new(TestRenderer::default(), root_key, move || {
        callbackless_root_render_probe(Rc::clone(&render_count_for_app));
    });

    assert_eq!(
        render_count.get(),
        2,
        "AppShell::new must replay pending root renders before publishing the first frame"
    );
    assert!(
        !shell.composition.take_root_render_request(),
        "initial shell setup should not leave a pending root render request behind"
    );

    let texts = layout_tree_texts(shell.layout_tree().expect("layout tree available"));
    assert!(
        texts.iter().any(|text| text == "Render 2"),
        "initial frame should reflect the replayed root render, got {texts:?}"
    );
}

#[test]
fn first_update_after_construction_reports_no_visual_work() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let rebuilds = Rc::new(Cell::new(0));
    let mut shell = AppShell::new(
        CountingRenderer::new(Rc::clone(&rebuilds)),
        root_key,
        box_content,
    );

    assert_eq!(
        rebuilds.get(),
        1,
        "construction must build the scene exactly once"
    );

    let first = shell.update();
    assert!(
        !first.visual_changed,
        "the first post-construction update finds a clean tree and reports no visual work"
    );
    assert_eq!(
        rebuilds.get(),
        1,
        "the first update must not rebuild the already-built scene"
    );
}

#[test]
fn an_open_frame_await_asks_for_ticks_but_not_for_pixels() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let rebuilds = Rc::new(Cell::new(0));
    let mut shell = AppShell::new(
        CountingRenderer::new(Rc::clone(&rebuilds)),
        root_key,
        silent_frame_loop_content,
    );

    for _ in 0..3 {
        shell.update();
    }
    rebuilds.set(0);

    let result = shell.update();

    assert!(
        shell.needs_update(),
        "an armed frame callback still owes the app a tick"
    );
    assert!(
        !result.visual_changed,
        "a frame loop that writes no state changes no pixels"
    );
    assert!(
        !shell.needs_redraw(),
        "and so must not ask the platform to present a frame identical to the last"
    );
    assert_eq!(
        rebuilds.get(),
        0,
        "a silent tick must not rebuild the scene"
    );
}

#[test]
fn clean_frame_reports_no_visual_work_with_dev_overlay_enabled() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let rebuilds = Rc::new(Cell::new(0));
    let mut shell = AppShell::new(
        CountingRenderer::new(Rc::clone(&rebuilds)),
        root_key,
        box_content,
    );

    let options = DevOptions {
        fps_counter: true,
        ..Default::default()
    };
    shell.set_dev_options(options);
    assert!(
        shell.debug_enter_app_context(cranpose_ui::peek_render_invalidation),
        "dev option changes must request a renderer update"
    );

    let overlay_result = shell.update();
    assert!(
        overlay_result.visual_changed,
        "enabling the dev overlay must produce one visual scene update"
    );

    rebuilds.set(0);
    let clean_result = shell.update();

    assert!(
        !clean_result.visual_changed,
        "clean app-shell frames must not be presented as visual work"
    );
    assert_eq!(
        rebuilds.get(),
        0,
        "clean frames must not rebuild the scene just to refresh FPS text"
    );
    assert!(
        !shell.needs_redraw(),
        "the overlay must not keep the shell dirty after a clean frame"
    );
}

#[test]
fn dev_overlay_reuses_text_inside_refresh_window_and_updates_after_it() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let rebuilds = Rc::new(Cell::new(0));
    let overlay_texts = Rc::new(RefCell::new(Vec::new()));
    let mut shell = AppShell::new(
        CountingRenderer::with_overlay_texts(Rc::clone(&rebuilds), Rc::clone(&overlay_texts)),
        root_key,
        box_content,
    );

    let options = DevOptions {
        fps_counter: true,
        frame_pacing_controls: true,
        ..Default::default()
    };
    shell.set_dev_options(options);
    shell.update();
    overlay_texts.borrow_mut().clear();

    shell.reset_fps_stats();
    shell.record_presented_frame_for_test(0, 1_000_000);
    shell.record_presented_frame_for_test(20_000_000, 21_000_000);
    shell.debug_enter_app_context(cranpose_ui::request_render_invalidation);
    shell.update();
    let slow_overlay = overlay_texts
        .borrow()
        .last()
        .expect("slow overlay text should be drawn")
        .clone();
    assert!(
        slow_overlay.starts_with("50 FPS"),
        "test setup should draw the slow frame history first: {slow_overlay}"
    );

    overlay_texts.borrow_mut().clear();
    for index in 1..=60u64 {
        let started = 20_000_000 + index * 4_000_000;
        shell.record_presented_frame_for_test(started, started + 1_000_000);
    }
    shell.debug_enter_app_context(cranpose_ui::request_render_invalidation);
    shell.update();
    let cached_overlay = overlay_texts
        .borrow()
        .last()
        .expect("cached overlay text should be drawn")
        .clone();
    assert_eq!(
        cached_overlay, slow_overlay,
        "the dev overlay must not rebuild dynamic FPS text on every visual frame"
    );

    overlay_texts.borrow_mut().clear();
    shell.dev_overlay_last_refresh = Some(web_time::Instant::now() - Duration::from_millis(300));
    shell.debug_enter_app_context(cranpose_ui::request_render_invalidation);
    shell.update();
    let fast_overlay = overlay_texts
        .borrow()
        .last()
        .expect("refreshed overlay text should be drawn")
        .clone();
    assert!(
        fast_overlay.starts_with("250 FPS"),
        "the dev overlay must report current FPS stats after the refresh window: slow={slow_overlay:?} fast={fast_overlay:?}"
    );
}

#[test]
fn a_press_on_a_pacing_control_changes_the_mode_whatever_produced_the_press() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, box_content);
    shell.set_viewport(1200.0, 800.0);
    shell.set_dev_options(DevOptions {
        fps_counter: true,
        frame_pacing_controls: true,
        ..Default::default()
    });
    shell.update();

    assert_eq!(shell.frame_pacing_mode(), FramePacingMode::Vsync);
    let (x, y) = shell
        .dev_overlay_control_center(FramePacingMode::NoVsync)
        .expect("the overlay must lay out a control for every pacing mode");

    shell.set_cursor(x, y);
    assert!(
        shell.pointer_pressed(),
        "a press on a pacing control is the shell's to take"
    );
    shell.pointer_released_at_position(x, y);

    assert_eq!(shell.frame_pacing_mode(), FramePacingMode::NoVsync);
    assert!(
        shell.needs_redraw(),
        "the overlay has to redraw to show which mode is now selected"
    );

    shell.update();
    shell.set_cursor(x, y + 400.0);
    shell.pointer_pressed();
    shell.pointer_released_at_position(x, y + 400.0);
    assert_eq!(shell.frame_pacing_mode(), FramePacingMode::NoVsync);
}

#[test]
fn pacing_controls_stay_out_of_the_way_when_they_are_switched_off() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, box_content);
    shell.set_viewport(1200.0, 800.0);
    shell.set_dev_options(DevOptions {
        fps_counter: true,
        ..Default::default()
    });
    shell.update();

    assert!(
        shell
            .dev_overlay_control_center(FramePacingMode::NoVsync)
            .is_none(),
        "an overlay without pacing controls must not lay any out"
    );
    shell.set_cursor(1000.0, 12.0);
    shell.pointer_pressed();
    assert_eq!(shell.frame_pacing_mode(), FramePacingMode::Vsync);
}

#[test]
fn pointer_invalidation_without_scene_changes_skips_scene_rebuild() {
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
    let app_context = Rc::clone(&shell.app_context);
    app_context.enter(|| {
        cranpose_ui::schedule_pointer_repass(root);
        cranpose_ui::request_pointer_invalidation();
    });

    shell.process_frame();

    assert_eq!(
        rebuilds.get(),
        0,
        "pure pointer invalidation should refresh dispatch state without rebuilding the visual scene"
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
    let app_context = Rc::clone(&shell.app_context);
    app_context.enter(|| {
        cranpose_ui::schedule_focus_invalidation(root);
        cranpose_ui::request_focus_invalidation();
    });

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
fn semantics_snapshot_revision_is_still_when_nothing_semantic_changed() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, semantics_content);

    let disabled = shell.semantics_snapshot_revision();
    assert_eq!(
        disabled,
        shell.semantics_snapshot_revision(),
        "with semantics off the revision must not move on its own"
    );

    shell.set_semantics_enabled(true);
    shell.process_frame();
    assert_ne!(
        disabled,
        shell.semantics_snapshot_revision(),
        "enabling semantics must move the revision"
    );

    let _ = shell.semantics_tree();
    let settled = shell.semantics_snapshot_revision();
    shell.process_frame();
    assert_eq!(
        settled,
        shell.semantics_snapshot_revision(),
        "a frame that changed nothing semantic must keep the revision still"
    );
}

#[test]
fn lazy_item_animation_updates_semantics_after_app_shell_frame() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, AppShellAnimatedLazyItem);

    shell.set_semantics_enabled(true);
    shell.update_at_frame_time_nanos(0);

    let initial = first_semantics_description_with_prefix(&mut shell, "Lazy Pulse:")
        .expect("initial lazy pulse semantics");

    for frame in 1..80 {
        shell.update_at_frame_time_nanos(frame * 16_666_667);
        let current = first_semantics_description_with_prefix(&mut shell, "Lazy Pulse:")
            .expect("lazy pulse semantics after frame");
        if current != initial {
            return;
        }
    }

    panic!("lazy item animation semantics stayed frozen at {initial}");
}

#[test]
fn layout_tree_snapshot_is_built_on_demand() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, semantics_content);

    assert!(
        shell.layout_tree.is_none(),
        "layout should render from retained node state without eagerly caching a LayoutTree"
    );
    assert!(
        shell.layout_tree().is_some(),
        "debug and robot callers should still be able to request a LayoutTree snapshot"
    );
    assert!(
        shell.layout_tree.is_some(),
        "requested LayoutTree snapshot should be cached until the next layout pass"
    );
}

#[test]
fn scroll_to_item_updates_indicator_in_layout_tree_and_semantics_tree() {
    let _guard = test_guard();
    APP_SHELL_LAZY_LIST_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, || {
        AppShellScrollIndicatorLazyList();
    });

    shell.set_semantics_enabled(true);
    shell.update();

    let initial_layout_texts = layout_tree_texts(shell.layout_tree().expect("layout tree"));
    assert!(
        initial_layout_texts
            .iter()
            .any(|text| text == "First visible 0"),
        "expected initial layout text, got {initial_layout_texts:?}"
    );
    let initial_semantics = semantics_tree_descriptions(shell.semantics_tree().expect("semantics"));
    assert!(
        initial_semantics
            .iter()
            .any(|text| text == "First visible 0"),
        "expected initial semantics text, got {initial_semantics:?}"
    );

    let list_state = APP_SHELL_LAZY_LIST_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("lazy list state registered");
    list_state.scroll_to_item(20, 0.0);

    for _ in 0..8 {
        if !shell.needs_redraw() && !shell.has_active_animations() {
            break;
        }
        shell.update();
    }

    let layout_texts = layout_tree_texts(shell.layout_tree().expect("layout tree"));
    let semantics = semantics_tree_descriptions(shell.semantics_tree().expect("semantics"));

    assert!(
        layout_texts.iter().any(|text| text == "First visible 20"),
        "expected layout tree text to reflect scroll target, got {layout_texts:?}"
    );
    assert!(
        semantics.iter().any(|text| text == "First visible 20"),
        "expected semantics tree text to reflect scroll target, got {semantics:?}"
    );
}

#[test]
fn scroll_to_item_updates_first_visible_when_sibling_stats_scope_is_present() {
    let _guard = test_guard();
    APP_SHELL_LAZY_LIST_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, || {
        AppShellSiblingIndicatorsLazyList();
    });

    shell.update();

    let initial_layout_texts = layout_tree_texts(shell.layout_tree().expect("layout tree"));
    assert!(
        initial_layout_texts
            .iter()
            .any(|text| text == "Child first visible 0"),
        "expected initial child indicator text, got {initial_layout_texts:?}"
    );

    let list_state = APP_SHELL_LAZY_LIST_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("lazy list state registered");
    list_state.scroll_to_item(20, 0.0);

    for _ in 0..8 {
        if !shell.needs_redraw() && !shell.has_active_animations() {
            break;
        }
        shell.update();
    }

    let layout_texts = layout_tree_texts(shell.layout_tree().expect("layout tree"));
    assert!(
        layout_texts
            .iter()
            .any(|text| text == "Child first visible 20"),
        "expected child indicator text to reflect scroll target with sibling stats scope present, got {layout_texts:?}"
    );
}

#[test]
fn scroll_to_item_updates_first_visible_under_callbackless_with_key_parent() {
    let _guard = test_guard();
    APP_SHELL_LAZY_LIST_STATE.with(|slot| slot.borrow_mut().take());
    APP_SHELL_ACTIVE_TAB_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, || {
        AppShellKeyedSiblingIndicatorsRoot();
    });

    shell.update();

    let list_state = APP_SHELL_LAZY_LIST_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("lazy list state registered");
    list_state.scroll_to_item(20, 0.0);

    for _ in 0..8 {
        if !shell.needs_redraw() && !shell.has_active_animations() {
            break;
        }
        shell.update();
    }

    let layout_texts = layout_tree_texts(shell.layout_tree().expect("layout tree"));
    assert!(
        layout_texts
            .iter()
            .any(|text| text == "Child first visible 20"),
        "expected child indicator text to reflect scroll target under keyed callbackless parent, got {layout_texts:?}"
    );
}

#[test]
fn scroll_to_item_updates_first_visible_after_switching_to_keyed_lazy_list_branch() {
    let _guard = test_guard();
    APP_SHELL_LAZY_LIST_STATE.with(|slot| slot.borrow_mut().take());
    APP_SHELL_ACTIVE_TAB_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, || {
        AppShellSwitchingKeyedLazyListRoot();
    });

    shell.update();

    let active = APP_SHELL_ACTIVE_TAB_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("active branch state registered");
    active.set(1);

    for _ in 0..8 {
        if !shell.needs_redraw() && !shell.has_active_animations() {
            break;
        }
        shell.update();
    }

    let list_state = APP_SHELL_LAZY_LIST_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("lazy list state registered after branch switch");
    list_state.scroll_to_item(20, 0.0);

    for _ in 0..8 {
        if !shell.needs_redraw() && !shell.has_active_animations() {
            break;
        }
        shell.update();
    }

    let layout_texts = layout_tree_texts(shell.layout_tree().expect("layout tree"));
    assert!(
        layout_texts
            .iter()
            .any(|text| text == "Child first visible 20"),
        "expected child indicator text to reflect scroll target after keyed branch switch, got {layout_texts:?}"
    );
}

#[test]
fn scroll_to_item_updates_first_visible_when_variable_height_stats_also_change() {
    let _guard = test_guard();
    APP_SHELL_LAZY_LIST_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, || {
        AppShellVariableHeightSiblingIndicatorsLazyList();
    });

    shell.update();

    let initial_layout_texts = layout_tree_texts(shell.layout_tree().expect("layout tree"));
    assert!(
        initial_layout_texts
            .iter()
            .any(|text| text == "Child first visible 0"),
        "expected initial child indicator text, got {initial_layout_texts:?}"
    );

    let list_state = APP_SHELL_LAZY_LIST_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("lazy list state registered");
    list_state.scroll_to_item(50, 0.0);

    for _ in 0..8 {
        if !shell.needs_redraw() && !shell.has_active_animations() {
            break;
        }
        shell.update();
    }

    let layout_texts = layout_tree_texts(shell.layout_tree().expect("layout tree"));
    assert!(
        layout_texts
            .iter()
            .any(|text| text == "Child first visible 50"),
        "expected child indicator text to reflect scroll target when stats sibling also changes, got {layout_texts:?}"
    );
}

#[test]
fn scroll_to_item_updates_first_visible_when_scroll_also_composes_lifecycle_items() {
    let _guard = test_guard();
    APP_SHELL_LAZY_LIST_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, || {
        AppShellLifecycleIndicatorsLazyList();
    });

    shell.update();

    let list_state = APP_SHELL_LAZY_LIST_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("lazy list state registered");
    list_state.scroll_to_item(50, 0.0);

    for _ in 0..12 {
        if !shell.needs_redraw() && !shell.has_active_animations() {
            break;
        }
        shell.update();
    }

    let layout_texts = layout_tree_texts(shell.layout_tree().expect("layout tree"));
    assert!(
        layout_texts
            .iter()
            .any(|text| text == "Child first visible 50"),
        "expected child indicator text to reflect scroll target when item composition also invalidates sibling state, got {layout_texts:?}"
    );
}

#[test]
fn draw_refresh_scope_only_contains_dirty_ancestors() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, nested_branch_content);

    shell.update();
    let layout_tree = shell.layout_tree().expect("expected layout tree");
    let root = node_id_at_path(layout_tree.root(), &[0]);
    let left = node_id_at_path(layout_tree.root(), &[0, 0]);
    let left_leaf = node_id_at_path(layout_tree.root(), &[0, 0, 0]);
    let right = node_id_at_path(layout_tree.root(), &[0, 1]);
    let right_leaf = node_id_at_path(layout_tree.root(), &[0, 1, 0]);

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
    assert!(
        shell.layout_tree().is_some(),
        "query helpers should be able to request a measured layout tree"
    );

    let cached_tree_ptr = shell
        .layout_tree
        .as_ref()
        .map(|tree| tree as *const cranpose_ui::LayoutTree)
        .expect("expected retained layout tree");

    let (root_id, root_bounds, left_leaf_id, left_leaf_bounds, right_id, right_bounds) = {
        let layout_tree = shell
            .layout_tree
            .as_ref()
            .expect("expected cached layout tree");
        let root = layout_box_at_path(layout_tree.root(), &[]);
        let left_leaf = layout_box_at_path(layout_tree.root(), &[0, 0, 0]);
        let right = layout_box_at_path(layout_tree.root(), &[0, 1]);

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
    assert_eq!(
        shell
            .layout_tree
            .as_ref()
            .map(|tree| tree as *const cranpose_ui::LayoutTree),
        Some(cached_tree_ptr),
        "root_layout_size should reuse the retained layout tree cache",
    );
    assert_eq!(shell.node_layout_bounds(root_id), Some(root_bounds));
    assert_eq!(
        shell.node_layout_bounds(left_leaf_id),
        Some(left_leaf_bounds)
    );
    assert_eq!(shell.node_layout_bounds(right_id), Some(right_bounds));
    assert_eq!(
        shell
            .layout_tree
            .as_ref()
            .map(|tree| tree as *const cranpose_ui::LayoutTree),
        Some(cached_tree_ptr),
        "layout bound queries should not rebuild the layout tree",
    );
}

#[composable]
fn app_shell_scrollable_test_tab(label: &'static str) {
    let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
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
fn app_shell_wheel_scroll_probe() {
    let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
    APP_SHELL_WHEEL_SCROLL_STATE.with(|slot| {
        *slot.borrow_mut() = Some(scroll_state);
    });

    Column(
        Modifier::empty()
            .fill_max_size()
            .vertical_scroll(scroll_state, false),
        ColumnSpec::default(),
        move || {
            Text(
                "Wheel scroll probe top",
                Modifier::empty(),
                TextStyle::default(),
            );
            Spacer(Size {
                width: 0.0,
                height: 900.0,
            });
            Text(
                "Wheel scroll probe bottom",
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}

thread_local! {
    static APP_SHELL_ZOOM_STATE: RefCell<Option<cranpose_ui::ZoomState>> =
        const { RefCell::new(None) };
}

#[composable]
fn app_shell_zoomable_probe() {
    let zoom_state = cranpose_core::remember(cranpose_ui::ZoomState::new).with(|state| *state);
    APP_SHELL_ZOOM_STATE.with(|slot| {
        *slot.borrow_mut() = Some(zoom_state);
    });

    Box(
        Modifier::empty().fill_max_size().zoomable(zoom_state),
        BoxSpec::default(),
        || {
            Text("Zoomable probe", Modifier::empty(), TextStyle::default());
        },
    );
}

#[composable]
fn app_shell_tall_fling_scroll_probe() {
    let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
    APP_SHELL_WHEEL_SCROLL_STATE.with(|slot| {
        *slot.borrow_mut() = Some(scroll_state);
    });

    Column(
        Modifier::empty()
            .fill_max_size()
            .vertical_scroll(scroll_state, false),
        ColumnSpec::default(),
        move || {
            Text("Fling probe top", Modifier::empty(), TextStyle::default());
            Spacer(Size {
                width: 0.0,
                height: 60_000.0,
            });
            Text(
                "Fling probe bottom",
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}

#[composable]
fn app_shell_consumed_child_drag_scroll_probe() {
    let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
    APP_SHELL_WHEEL_SCROLL_STATE.with(|slot| {
        *slot.borrow_mut() = Some(scroll_state);
    });

    Column(
        Modifier::empty()
            .fill_max_size()
            .vertical_scroll(scroll_state, false),
        ColumnSpec::default(),
        move || {
            Box(
                Modifier::empty()
                    .size(Size {
                        width: 180.0,
                        height: 120.0,
                    })
                    .pointer_input((), move |scope: PointerInputScope| async move {
                        scope
                            .await_pointer_event_scope(|await_scope| async move {
                                loop {
                                    let event = await_scope.await_pointer_event().await;
                                    match event.kind {
                                        PointerEventKind::Down | PointerEventKind::Move => {
                                            event.consume();
                                        }
                                        PointerEventKind::Up
                                        | PointerEventKind::Cancel
                                        | PointerEventKind::Scroll
                                        | PointerEventKind::Zoom
                                        | PointerEventKind::RotaryScrollPre
                                        | PointerEventKind::RotaryScroll
                                        | PointerEventKind::Enter
                                        | PointerEventKind::Exit => {}
                                    }
                                }
                            })
                            .await;
                    }),
                BoxSpec::default(),
                || {},
            );
            Spacer(Size {
                width: 0.0,
                height: 900.0,
            });
            Text(
                "Consumed child drag bottom",
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}

#[composable]
fn app_shell_horizontal_clickable_wheel_scroll_probe() {
    let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
    APP_SHELL_WHEEL_SCROLL_STATE.with(|slot| {
        *slot.borrow_mut() = Some(scroll_state);
    });

    Row(
        Modifier::empty()
            .fill_max_width()
            .height(72.0)
            .clip_to_bounds()
            .horizontal_scroll(scroll_state, false),
        RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            for index in 0..8 {
                Button(
                    Modifier::empty().width(112.0).height(48.0),
                    ButtonSpec::default(),
                    || {},
                    move || {
                        Text(
                            format!("Tab {index}"),
                            Modifier::empty(),
                            TextStyle::default(),
                        );
                    },
                );
            }
        },
    );
}

#[composable]
fn app_shell_scrollable_wrapper(content: impl FnMut() + 'static) {
    let scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
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
    let pointer_position = rememberMutableStateOf(Point::default);
    let pointer_down = rememberMutableStateOf(|| false);
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
                                            | PointerEventKind::Zoom
                                            | PointerEventKind::RotaryScrollPre
                                            | PointerEventKind::RotaryScroll
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
                            ButtonSpec::default(),
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
    let request_counter = rememberMutableStateOf(|| 0u64);
    let status = rememberMutableStateOf(|| "Idle".to_string());
    let request_key = request_counter.get();

    launched_effect_async_impl(
        location_key(file!(), line!(), column!()),
        TaskSite::new(file!(), line!()),
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
            cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state),
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
    let counter = rememberMutableStateOf(|| 7i32);
    let provided = counter.get();
    Column(
        Modifier::empty().fill_max_size().vertical_scroll(
            cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state),
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
    let active_tab = rememberMutableStateOf(|| 3i32);
    let counter = rememberMutableStateOf(|| 0i32);
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
    let active_tab = rememberMutableStateOf(|| 0i32);
    let counter = rememberMutableStateOf(|| 0i32);
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
    let active_tab = rememberMutableStateOf(|| 0i32);
    let counter = rememberMutableStateOf(|| 0i32);

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
                    ButtonSpec::default(),
                    move || counter_tab.set_value(0),
                    || {
                        Text("Counter App", Modifier::empty(), TextStyle::default());
                    },
                );
                Button(
                    Modifier::empty().padding(8.0),
                    ButtonSpec::default(),
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
                    ButtonSpec::default(),
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
    let counter = rememberMutableStateOf(|| 0i32);
    let wave_state = rememberMutableStateOf(|| 0.35f32);
    let pointer_position = rememberMutableStateOf(Point::default);
    let pointer_down = rememberMutableStateOf(|| false);
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
                                                        | PointerEventKind::Zoom
                                                        | PointerEventKind::RotaryScrollPre
                                                        | PointerEventKind::RotaryScroll
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
                                            ButtonSpec::default(),
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
    let active_tab = rememberMutableStateOf(|| 0i32);

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            let counter_tab = active_tab;
            let composition_local_tab = active_tab;
            Row(Modifier::empty(), RowSpec::default(), move || {
                Button(
                    Modifier::empty().padding(8.0),
                    ButtonSpec::default(),
                    move || counter_tab.set_value(0),
                    || {
                        Text("Counter App", Modifier::empty(), TextStyle::default());
                    },
                );
                Button(
                    Modifier::empty().padding(8.0),
                    ButtonSpec::default(),
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
    let counter = rememberMutableStateOf(|| 0i32);
    let wave_state = rememberMutableStateOf(|| 0.35f32);
    let pointer_position = rememberMutableStateOf(Point::default);
    let pointer_down = rememberMutableStateOf(|| false);
    let async_message =
        rememberMutableStateOf(|| "Tap \"Fetch async value\" to run background work".to_string());
    let fetch_request = rememberMutableStateOf(|| 0u64);
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
                                            | PointerEventKind::Zoom
                                            | PointerEventKind::RotaryScrollPre
                                            | PointerEventKind::RotaryScroll
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
                                    ButtonSpec::default(),
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
                                ButtonSpec::default(),
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
                                ButtonSpec::default(),
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
                        ButtonSpec::default(),
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
    let active_tab = rememberMutableStateOf(|| 0i32);

    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            let counter_tab = active_tab;
            let composition_local_tab = active_tab;
            Row(Modifier::empty(), RowSpec::default(), move || {
                Button(
                    Modifier::empty().padding(8.0),
                    ButtonSpec::default(),
                    move || counter_tab.set_value(0),
                    || {
                        Text("Counter App", Modifier::empty(), TextStyle::default());
                    },
                );
                Button(
                    Modifier::empty().padding(8.0),
                    ButtonSpec::default(),
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

    let active_tab = rememberMutableStateOf(|| 0i32);

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
            ButtonSpec::default(),
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
                cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
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
        ButtonSpec::default(),
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

    let tabs_scroll_state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
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
    let active_tab = rememberMutableStateOf(|| 0i32);

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
    let phase_state = rememberMutableStateOf(|| 0.35f32);

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
                ButtonSpec::default(),
                move || phase_state.set_value(0.8),
                || {
                    Text("Drive wave", Modifier::empty(), TextStyle::default());
                },
            );
        },
    );
}

fn app_shell_animated_draw_tab_host() {
    let active_tab = rememberMutableStateOf(|| 0i32);
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
fn parent_pointer_listener_releases_after_child_button_click_recomposes() {
    let _guard = test_guard();
    APP_SHELL_COUNTER_STATE.with(|slot| slot.borrow_mut().take());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_interactive_clickable_tab_host,
    );

    shell.set_semantics_enabled(true);
    shell.update();

    let increment = find_layout_box_with_text(
        shell.layout_tree().expect("counter layout tree").root(),
        "Increment",
    )
    .expect("increment button in layout tree");
    let button_x = increment.rect.x + increment.rect.width * 0.5;
    let button_y = increment.rect.y + increment.rect.height * 0.5;

    assert!(shell.set_cursor(button_x, button_y));
    shell.update();
    assert!(shell.pointer_pressed(), "pointer down should hit increment");
    shell.update();

    let pressed_texts = layout_tree_texts(shell.layout_tree().expect("pressed counter layout"));
    assert!(
        pressed_texts.iter().any(|text| text.contains("down=true")),
        "parent pointer listener did not observe Down before child click: {pressed_texts:?}",
    );

    assert!(
        shell.pointer_released(),
        "pointer up should reach captured child and parent targets"
    );
    shell.update();

    let released_texts = layout_tree_texts(shell.layout_tree().expect("released counter layout"));
    assert!(
        released_texts
            .iter()
            .any(|text| text.contains("Counter value 1")),
        "button click did not update counter after release: {released_texts:?}",
    );
    assert!(
        released_texts
            .iter()
            .any(|text| text.contains("down=false")),
        "parent pointer listener stayed pressed after child button release: {released_texts:?}",
    );
}

const PRESSABLE_CANVAS_NORMAL_COLOR: Color = Color(0.1, 0.2, 0.8, 1.0);
const PRESSABLE_CANVAS_PRESSED_COLOR: Color = Color(0.8, 0.2, 0.1, 1.0);

#[composable]
fn app_shell_pressable_canvas_content() {
    let is_pressed = rememberMutableStateOf(|| false);
    let color = if is_pressed.get() {
        PRESSABLE_CANVAS_PRESSED_COLOR
    } else {
        PRESSABLE_CANVAS_NORMAL_COLOR
    };
    cranpose_ui::Canvas(
        Modifier::empty().size_points(60.0, 30.0).pointer_input(
            (),
            move |scope: PointerInputScope| async move {
                scope
                    .await_pointer_event_scope(|await_scope| async move {
                        loop {
                            let event = await_scope.await_pointer_event().await;
                            match event.kind {
                                PointerEventKind::Down => {
                                    is_pressed.set(true);
                                    event.consume();
                                }
                                PointerEventKind::Up | PointerEventKind::Cancel => {
                                    is_pressed.set(false);
                                    event.consume();
                                }
                                _ => {}
                            }
                        }
                    })
                    .await;
            },
        ),
        move |scope| {
            scope.draw_rect(Brush::solid(color));
        },
    );
}

fn graph_rect_colors(graph: &cranpose_render_common::graph::RenderGraph) -> Vec<Color> {
    fn collect_node(node: &cranpose_render_common::graph::RenderNode, out: &mut Vec<Color>) {
        match node {
            cranpose_render_common::graph::RenderNode::Primitive(entry) => {
                if let cranpose_render_common::graph::PrimitiveNode::Draw(draw) = &entry.node
                    && let DrawPrimitive::Rect {
                        brush: cranpose_ui_graphics::Brush::Solid(color),
                        ..
                    } = &draw.primitive
                {
                    out.push(*color);
                }
            }
            cranpose_render_common::graph::RenderNode::DrawRun(run) => {
                for primitive in run.primitives.iter() {
                    if let DrawPrimitive::Rect {
                        brush: cranpose_ui_graphics::Brush::Solid(color),
                        ..
                    } = primitive
                    {
                        out.push(*color);
                    }
                }
            }
            cranpose_render_common::graph::RenderNode::Layer(layer) => {
                collect_layer(layer, out);
            }
        }
    }

    fn collect_layer(layer: &cranpose_render_common::graph::LayerNode, out: &mut Vec<Color>) {
        for child in &layer.children {
            collect_node(child, out);
        }
    }

    let mut colors = Vec::new();
    collect_layer(&graph.root, &mut colors);
    colors
}

#[test]
fn canvas_pressed_state_draws_on_pointer_down_before_release() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_pressable_canvas_content,
    );

    shell.update();
    let colors = graph_rect_colors(shell.renderer.scene.graph.as_ref().expect("initial graph"));
    assert!(
        colors.contains(&PRESSABLE_CANVAS_NORMAL_COLOR),
        "canvas should draw the normal color before any press: {colors:?}"
    );
    assert!(
        !colors.contains(&PRESSABLE_CANVAS_PRESSED_COLOR),
        "canvas must not draw the pressed color before any press: {colors:?}"
    );

    assert!(
        shell.set_cursor(30.0, 15.0),
        "cursor should hover the pressable canvas"
    );
    shell.update();
    assert!(
        shell.pointer_pressed(),
        "pointer down should hit the pressable canvas"
    );
    shell.update();

    let colors = graph_rect_colors(shell.renderer.scene.graph.as_ref().expect("pressed graph"));
    assert!(
        colors.contains(&PRESSABLE_CANVAS_PRESSED_COLOR),
        "canvas must draw the pressed color after pointer down, before release: {colors:?}"
    );

    assert!(
        shell.pointer_released(),
        "pointer up should dispatch to the pressable canvas"
    );
    shell.update();

    let colors = graph_rect_colors(shell.renderer.scene.graph.as_ref().expect("released graph"));
    assert!(
        colors.contains(&PRESSABLE_CANVAS_NORMAL_COLOR),
        "canvas should draw the normal color again after release: {colors:?}"
    );
    assert!(
        !colors.contains(&PRESSABLE_CANVAS_PRESSED_COLOR),
        "canvas must not keep the pressed color after release: {colors:?}"
    );
}

thread_local! {
    static APP_SHELL_POINTER_SCOPE: RefCell<Option<PointerInputScope>> = const { RefCell::new(None) };
    static APP_SHELL_POINTER_SCOPE_EVENTS: RefCell<Vec<(Size, Point)>> =
        const { RefCell::new(Vec::new()) };
}

#[composable]
fn app_shell_pointer_scope_size_probe() {
    Box(
        Modifier::empty().fill_max_size().pointer_input(
            (),
            move |scope: PointerInputScope| async move {
                APP_SHELL_POINTER_SCOPE.with(|slot| {
                    *slot.borrow_mut() = Some(scope.clone());
                });
                scope
                    .await_pointer_event_scope(|await_scope| async move {
                        loop {
                            let event = await_scope.await_pointer_event().await;
                            if event.kind == PointerEventKind::Down {
                                APP_SHELL_POINTER_SCOPE_EVENTS.with(|slot| {
                                    slot.borrow_mut().push((await_scope.size(), event.position));
                                });
                            }
                        }
                    })
                    .await;
            },
        ),
        BoxSpec::default(),
        || {},
    );
}

#[test]
fn pointer_input_scope_reports_node_size_in_running_shell() {
    let _guard = test_guard();
    APP_SHELL_POINTER_SCOPE.with(|slot| {
        slot.borrow_mut().take();
    });
    APP_SHELL_POINTER_SCOPE_EVENTS.with(|slot| slot.borrow_mut().clear());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_pointer_scope_size_probe,
    );
    shell.set_viewport(408.0, 408.0);
    shell.update();

    let scope = APP_SHELL_POINTER_SCOPE
        .with(|slot| slot.borrow().clone())
        .expect("pointer input handler should have started");

    assert_eq!(
        scope.size(),
        Size {
            width: 408.0,
            height: 408.0
        },
        "scope.size() must report the node's size after the first frame, with no pointer event"
    );

    assert!(
        shell.set_cursor(204.0, 204.0),
        "cursor should hover the full-screen pointer surface"
    );
    shell.update();
    assert!(
        shell.pointer_pressed(),
        "pointer down should hit the surface"
    );
    shell.update();

    let (event_size, event_position) = APP_SHELL_POINTER_SCOPE_EVENTS
        .with(|slot| slot.borrow().first().copied())
        .expect("handler should have received the pointer down");
    assert_eq!(
        event_size,
        Size {
            width: 408.0,
            height: 408.0
        },
        "AwaitPointerEventScope::size() must report the node's size"
    );
    assert!(
        (event_position.x - event_size.width * 0.5).abs() < 0.5
            && (event_position.y - event_size.height * 0.5).abs() < 0.5,
        "a centre tap must land at size/2 in scope coordinates, got {event_position:?} for {event_size:?}"
    );

    shell.set_viewport(300.0, 200.0);
    shell.update();
    assert_eq!(
        scope.size(),
        Size {
            width: 300.0,
            height: 200.0
        },
        "scope.size() must track viewport-driven resizes"
    );

    APP_SHELL_POINTER_SCOPE.with(|slot| {
        slot.borrow_mut().take();
    });
    APP_SHELL_POINTER_SCOPE_EVENTS.with(|slot| slot.borrow_mut().clear());
}

thread_local! {
    static APP_SHELL_ROUTER_ARENA: RefCell<Option<MutableState<bool>>> = const { RefCell::new(None) };
    static APP_SHELL_ROUTER_MENU_DOWNS: Cell<u32> = const { Cell::new(0) };
    static APP_SHELL_ROUTER_ARENA_DOWNS: Cell<u32> = const { Cell::new(0) };
}

#[composable]
fn app_shell_router_branch_probe() {
    let arena = rememberMutableStateOf(|| false);
    APP_SHELL_ROUTER_ARENA.with(|slot| *slot.borrow_mut() = Some(arena));
    if arena.get() {
        Box(
            Modifier::empty().fill_max_size().pointer_input(
                (),
                move |scope: PointerInputScope| async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                if event.kind == PointerEventKind::Down {
                                    APP_SHELL_ROUTER_ARENA_DOWNS
                                        .with(|count| count.set(count.get() + 1));
                                }
                            }
                        })
                        .await;
                },
            ),
            BoxSpec::default(),
            || {},
        );
    } else {
        Box(
            Modifier::empty().fill_max_size().pointer_input(
                (),
                move |scope: PointerInputScope| async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                if event.kind == PointerEventKind::Down {
                                    APP_SHELL_ROUTER_MENU_DOWNS
                                        .with(|count| count.set(count.get() + 1));
                                }
                            }
                        })
                        .await;
                },
            ),
            BoxSpec::default(),
            || {},
        );
    }
}

#[test]
fn a_branch_switch_hands_the_gesture_to_the_branch_that_is_on_screen() {
    let _guard = test_guard();
    APP_SHELL_ROUTER_ARENA.with(|slot| {
        slot.borrow_mut().take();
    });
    APP_SHELL_ROUTER_MENU_DOWNS.with(|count| count.set(0));
    APP_SHELL_ROUTER_ARENA_DOWNS.with(|count| count.set(0));

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_router_branch_probe,
    );
    shell.set_viewport(408.0, 408.0);
    shell.update();

    let tap = |shell: &mut AppShell<HitGraphRenderer>| {
        shell.set_cursor(10.0, 10.0);
        shell.update();
        shell.set_cursor(204.0, 204.0);
        shell.update();
        assert!(shell.pointer_pressed(), "the surface must take the press");
        shell.update();
        assert!(
            shell.pointer_released(),
            "the surface must take the release"
        );
        shell.update();
    };

    tap(&mut shell);
    assert_eq!(
        APP_SHELL_ROUTER_MENU_DOWNS.with(|count| count.get()),
        1,
        "the menu owns the gesture while the menu is what is on screen"
    );

    APP_SHELL_ROUTER_ARENA
        .with(|slot| *slot.borrow())
        .expect("the probe publishes its route")
        .set(true);
    shell.update();

    tap(&mut shell);
    let menu = APP_SHELL_ROUTER_MENU_DOWNS.with(|count| count.get());
    let arena = APP_SHELL_ROUTER_ARENA_DOWNS.with(|count| count.get());
    assert_eq!(
        (menu, arena),
        (1, 1),
        "the arena must read the tap once the arena is what is on screen, and the \
         menu must stop reading gestures after it has left it (menu, arena)"
    );

    APP_SHELL_ROUTER_ARENA.with(|slot| {
        slot.borrow_mut().take();
    });
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
fn app_shell_single_frame_callback_returns_to_idle() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        one_shot_frame_request_content,
    );

    let mut saw_applied_frame = false;
    for _ in 0..8 {
        pump_like_robot(&mut shell);
        let texts = layout_tree_texts(shell.layout_tree().expect("layout tree available"));
        if texts.iter().any(|text| text == "Frame Applied") {
            saw_applied_frame = true;
            break;
        }
    }

    assert!(
        saw_applied_frame,
        "one-shot frame callback never completed: {:?}",
        layout_tree_texts(shell.layout_tree().expect("layout tree available"))
    );

    for _ in 0..3 {
        pump_like_robot(&mut shell);
    }

    assert!(
        !shell.has_active_animations(),
        "shell remained active after the one-shot frame callback completed"
    );
    assert!(
        !shell.needs_redraw(),
        "shell kept requesting redraw after the one-shot frame callback completed"
    );
}

#[test]
fn app_shell_hidden_frame_loop_tab_returns_to_idle() {
    let _guard = test_guard();
    APP_SHELL_ACTIVE_TAB_STATE.with(|slot| slot.borrow_mut().take());
    APP_SHELL_CONTINUOUS_FRAME_COUNT.with(|count| count.set(0));
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_continuous_then_static_tab_host,
    );

    for _ in 0..4 {
        pump_like_robot(&mut shell);
    }

    let active = APP_SHELL_ACTIVE_TAB_STATE
        .with(|slot| slot.borrow().as_ref().copied())
        .expect("active tab registered");
    let animated_frames = APP_SHELL_CONTINUOUS_FRAME_COUNT.with(Cell::get);
    assert!(
        animated_frames > 0,
        "test setup did not drive the animated tab"
    );

    active.set_value(1);
    shell.update();
    let texts = layout_tree_texts(shell.layout_tree().expect("static tab layout"));
    assert!(
        texts.iter().any(|text| text == "Static tab"),
        "static tab did not render after switching away: {texts:?}"
    );
    let after_switch_frame = APP_SHELL_CONTINUOUS_FRAME_COUNT.with(Cell::get);

    for _ in 0..4 {
        pump_like_robot(&mut shell);
    }

    let after_idle_pumps = APP_SHELL_CONTINUOUS_FRAME_COUNT.with(Cell::get);
    assert_eq!(
        after_idle_pumps, after_switch_frame,
        "hidden frame loop kept advancing after the tab was removed"
    );
    assert!(
        !shell.has_active_animations(),
        "hidden frame loop kept AppShell active"
    );
    assert!(
        !shell.needs_redraw(),
        "hidden frame loop kept requesting redraw"
    );
    assert!(
        !shell.frame_schedule().needs_frame,
        "idle shell must not schedule a platform frame after hidden frame loops settle"
    );
}

#[test]
fn app_shell_manual_frame_interval_advances_frame_clock_monotonically() {
    let _guard = test_guard();
    APP_SHELL_FRAME_TIME_RECORDS.with(|records| records.borrow_mut().clear());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        frame_time_recorder_content,
    );

    for _ in 0..4 {
        shell.update_after_frame_interval(Duration::from_nanos(16_666_667));
    }

    let records = APP_SHELL_FRAME_TIME_RECORDS.with(|records| records.borrow().clone());
    assert_eq!(records.len(), 2, "expected two recorded frame callbacks");
    assert!(
        records[1] > records[0],
        "manual frame pumping must advance animation time monotonically: {records:?}"
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
            primitive: DrawPrimitive::Rect { rect, brush, .. },
            ..
        } = op
            && *brush == Brush::solid(color)
        {
            return Some(rect.width);
        }
    }
    None
}

thread_local! {
    static POINTER_SOURCE_PROBE: RefCell<Option<Rc<Cell<PointerSource>>>> =
        const { RefCell::new(None) };
}

thread_local! {
    static POINTER_MODIFIERS_PROBE: RefCell<Option<Rc<Cell<Option<Modifiers>>>>> =
        const { RefCell::new(None) };
}

type PointerTimeSample = (PointerEventKind, Option<i64>, Option<u64>);

thread_local! {
    static POINTER_TIME_PROBE: RefCell<Option<Rc<RefCell<Vec<PointerTimeSample>>>>> =
        const { RefCell::new(None) };
}

fn app_shell_pointer_source_probe() {
    let captured = Rc::new(Cell::new(PointerSource::Unknown));
    POINTER_SOURCE_PROBE.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&captured)));
    Box(
        Modifier::empty()
            .size(Size {
                width: 200.0,
                height: 200.0,
            })
            .pointer_input((), move |scope: PointerInputScope| {
                let captured = Rc::clone(&captured);
                async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                if event.kind == PointerEventKind::Down {
                                    captured.set(event.source);
                                    event.consume();
                                }
                            }
                        })
                        .await;
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

fn app_shell_pointer_modifiers_probe() {
    let captured = Rc::new(Cell::new(None::<Modifiers>));
    POINTER_MODIFIERS_PROBE.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&captured)));
    Box(
        Modifier::empty()
            .size(Size {
                width: 200.0,
                height: 200.0,
            })
            .pointer_input((), move |scope: PointerInputScope| {
                let captured = Rc::clone(&captured);
                async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                if event.kind == PointerEventKind::Down {
                                    captured.set(event.modifiers);
                                    event.consume();
                                }
                            }
                        })
                        .await;
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

fn app_shell_pointer_time_probe() {
    let captured = Rc::new(RefCell::new(Vec::new()));
    POINTER_TIME_PROBE.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&captured)));
    Box(
        Modifier::empty()
            .size(Size {
                width: 200.0,
                height: 200.0,
            })
            .pointer_input((), move |scope: PointerInputScope| {
                let captured = Rc::clone(&captured);
                async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                captured.borrow_mut().push((
                                    event.kind,
                                    event.time_ms,
                                    event.animation_time_nanos,
                                ));
                                event.consume();
                            }
                        })
                        .await;
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

#[test]
fn shell_preserves_resolved_pointer_timestamps_during_dispatch() {
    let _guard = test_guard();
    POINTER_TIME_PROBE.with(|slot| slot.borrow_mut().take());

    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        app_shell_pointer_time_probe,
    );
    shell.set_buffer_size(200, 200);
    shell.set_viewport(200.0, 200.0);
    shell.update();

    let captured = POINTER_TIME_PROBE
        .with(|slot| slot.borrow().as_ref().map(Rc::clone))
        .expect("probe should expose captured samples");
    assert!(shell.set_cursor_at_event_time(
        50.0,
        50.0,
        PointerEventTime {
            platform_time_ms: Some(10),
            animation_time_nanos: 100,
        },
    ));
    assert!(shell.pointer_pressed_at_event_time(PointerEventTime {
        platform_time_ms: Some(11),
        animation_time_nanos: 110,
    }));
    assert!(shell.pointer_released_at_position_event_time(
        51.0,
        52.0,
        PointerEventTime {
            platform_time_ms: Some(12),
            animation_time_nanos: 120,
        },
    ));

    let samples = captured.borrow();
    assert!(samples.contains(&(PointerEventKind::Down, Some(11), Some(110))));
    assert!(samples.contains(&(PointerEventKind::Up, Some(12), Some(120))));
}

#[test]
fn shell_stamps_pointer_source_on_dispatched_events() {
    let _guard = test_guard();
    POINTER_SOURCE_PROBE.with(|slot| slot.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_pointer_source_probe,
    );
    shell.set_buffer_size(200, 200);
    shell.set_viewport(200.0, 200.0);
    shell.update();

    let captured = POINTER_SOURCE_PROBE
        .with(|slot| slot.borrow().as_ref().map(Rc::clone))
        .expect("probe should install its capture cell");

    shell.set_pointer_source(PointerSource::Touch);
    assert!(shell.set_cursor(50.0, 50.0));
    assert!(shell.pointer_pressed(), "down should hit the probe");
    assert_eq!(
        captured.get(),
        PointerSource::Touch,
        "shell must stamp the touch source onto the dispatched pointer event"
    );
    shell.pointer_released();

    shell.set_pointer_source(PointerSource::Mouse);
    assert!(shell.set_cursor(50.0, 50.0));
    assert!(shell.pointer_pressed());
    assert_eq!(
        captured.get(),
        PointerSource::Mouse,
        "shell must stamp the mouse source onto the dispatched pointer event"
    );
}

#[test]
fn shift_click_reaches_the_pointer_input_handler_through_the_shell() {
    let _guard = test_guard();
    POINTER_MODIFIERS_PROBE.with(|slot| slot.borrow_mut().take());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        app_shell_pointer_modifiers_probe,
    );
    shell.set_buffer_size(200, 200);
    shell.set_viewport(200.0, 200.0);
    shell.update();

    let captured = POINTER_MODIFIERS_PROBE
        .with(|slot| slot.borrow().as_ref().map(Rc::clone))
        .expect("probe should install its capture cell");

    assert_eq!(shell.modifiers(), None);
    assert!(shell.set_cursor(50.0, 50.0));
    assert!(shell.pointer_pressed(), "down should hit the probe");
    assert_eq!(
        captured.get(),
        None,
        "a press before set_modifiers must reach the handler as unreported, not as Modifiers::NONE"
    );
    shell.pointer_released();

    shell.set_modifiers(Modifiers {
        shift: true,
        ..Modifiers::NONE
    });
    assert_eq!(
        shell.modifiers(),
        Some(Modifiers {
            shift: true,
            ..Modifiers::NONE
        })
    );
    assert!(shell.set_cursor(50.0, 50.0));
    assert!(shell.pointer_pressed());
    let modifiers = captured
        .get()
        .expect("a press after set_modifiers must report Some(Modifiers)");
    assert!(
        modifiers.shift,
        "shift held at the platform must reach the pointer_input handler on the dispatched event"
    );
    assert!(!modifiers.ctrl);
    shell.pointer_released();

    shell.set_modifiers(Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    });
    assert!(shell.set_cursor(50.0, 50.0));
    assert!(shell.pointer_pressed());
    let modifiers = captured.get().expect("ctrl press must report Some");
    assert!(modifiers.ctrl);
    assert!(
        !modifiers.shift,
        "stale shift must not survive a modifier change"
    );
}

fn rotary_scene(targets: Vec<RecordingHitTarget>) -> RecordingScene {
    RecordingScene::with_hits(targets)
}

fn rotary_target(
    node_id: cranpose_core::NodeId,
    consume: bool,
    events: Rc<RefCell<Vec<PointerEvent>>>,
    capture_path: Vec<cranpose_core::NodeId>,
) -> RecordingHitTarget {
    RecordingHitTarget {
        node_id,
        consume,
        events,
        capture_path,
    }
}

#[test]
fn rotary_scrolled_runs_capture_then_bubble_passes() {
    let _guard = test_guard();
    let child_events = Rc::new(RefCell::new(Vec::new()));
    let ancestor_events = Rc::new(RefCell::new(Vec::new()));
    let scene = RecordingScene::with_hit_node_ids(
        vec![
            rotary_target(1, false, child_events.clone(), vec![1, 99]),
            rotary_target(99, false, ancestor_events.clone(), vec![99]),
        ],
        vec![1],
    );

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(ScrollDispatchRenderer::new(scene), root_key, empty_content);
    shell.set_cursor(20.0, 30.0);
    child_events.borrow_mut().clear();
    ancestor_events.borrow_mut().clear();

    let consumed = shell.rotary_scrolled(RotaryScrollEvent::new(-64.0, 0.0, 11));
    assert!(!consumed, "nothing consumed the event");

    let child_kinds: Vec<_> = child_events.borrow().iter().map(|e| e.kind).collect();
    let ancestor_kinds: Vec<_> = ancestor_events.borrow().iter().map(|e| e.kind).collect();
    assert_eq!(
        child_kinds,
        vec![
            PointerEventKind::RotaryScrollPre,
            PointerEventKind::RotaryScroll
        ]
    );
    assert_eq!(
        ancestor_kinds,
        vec![
            PointerEventKind::RotaryScrollPre,
            PointerEventKind::RotaryScroll
        ]
    );
}

#[test]
fn rotary_capture_pass_runs_root_to_leaf() {
    let _guard = test_guard();
    let child_events = Rc::new(RefCell::new(Vec::new()));
    let ancestor_events = Rc::new(RefCell::new(Vec::new()));
    let scene = RecordingScene::with_hit_node_ids(
        vec![
            rotary_target(1, false, child_events.clone(), vec![1, 99]),
            rotary_target(99, false, ancestor_events.clone(), vec![99]),
        ],
        vec![1],
    );

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(ScrollDispatchRenderer::new(scene), root_key, empty_content);
    shell.set_cursor(20.0, 30.0);
    child_events.borrow_mut().clear();
    ancestor_events.borrow_mut().clear();

    shell.rotary_scrolled(RotaryScrollEvent::new(-8.0, 0.0, 1));

    assert!(
        child_events
            .borrow()
            .iter()
            .any(|e| e.kind == PointerEventKind::RotaryScrollPre)
    );
    assert!(
        ancestor_events
            .borrow()
            .iter()
            .any(|e| e.kind == PointerEventKind::RotaryScroll)
    );
}

#[test]
fn rotary_capture_consumption_stops_the_bubble_pass() {
    let _guard = test_guard();
    let child_events = Rc::new(RefCell::new(Vec::new()));
    let ancestor_events = Rc::new(RefCell::new(Vec::new()));
    let scene = RecordingScene::with_hit_node_ids(
        vec![
            rotary_target(1, false, child_events.clone(), vec![1, 99]),
            rotary_target(99, true, ancestor_events.clone(), vec![99]),
        ],
        vec![1],
    );

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(ScrollDispatchRenderer::new(scene), root_key, empty_content);
    shell.set_cursor(20.0, 30.0);
    child_events.borrow_mut().clear();
    ancestor_events.borrow_mut().clear();

    let consumed = shell.rotary_scrolled(RotaryScrollEvent::new(-16.0, 0.0, 3));

    assert!(consumed, "ancestor consumed during capture");
    assert_eq!(ancestor_events.borrow().len(), 1);
    assert_eq!(
        child_events.borrow().len(),
        0,
        "capture consumption must stop propagation to descendants"
    );
}

#[test]
fn rotary_bubble_consumption_stops_at_the_consuming_node() {
    let _guard = test_guard();
    let child_events = Rc::new(RefCell::new(Vec::new()));
    let ancestor_events = Rc::new(RefCell::new(Vec::new()));
    let scene = RecordingScene::with_hit_node_ids(
        vec![
            rotary_target(1, true, child_events.clone(), vec![1, 99]),
            rotary_target(99, false, ancestor_events.clone(), vec![99]),
        ],
        vec![1],
    );

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(ScrollDispatchRenderer::new(scene), root_key, empty_content);
    shell.set_cursor(20.0, 30.0);
    child_events.borrow_mut().clear();
    ancestor_events.borrow_mut().clear();

    let consumed = shell.rotary_scrolled(RotaryScrollEvent::new(-16.0, 0.0, 5));

    assert!(consumed);
    assert_eq!(ancestor_events.borrow().len(), 1);
    assert_eq!(
        ancestor_events.borrow()[0].kind,
        PointerEventKind::RotaryScrollPre
    );
    assert_eq!(child_events.borrow().len(), 1);
}

#[test]
fn rotary_event_carries_its_payload_to_targets() {
    let _guard = test_guard();
    let events = Rc::new(RefCell::new(Vec::new()));
    let scene = rotary_scene(vec![rotary_target(1, true, events.clone(), vec![1])]);

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(ScrollDispatchRenderer::new(scene), root_key, empty_content);
    shell.set_cursor(20.0, 30.0);
    events.borrow_mut().clear();

    let rotary = RotaryScrollEvent::new(-64.0, 12.0, 4_242);
    assert!(shell.rotary_scrolled(rotary));

    let recorded = events.borrow();
    let event = recorded.first().expect("expected a rotary event");
    assert_eq!(event.rotary_scroll_event(), Some(rotary));
    assert_eq!(event.global_position, Point { x: 20.0, y: 30.0 });
}

#[test]
fn empty_rotary_events_are_dropped_before_dispatch() {
    let _guard = test_guard();
    let events = Rc::new(RefCell::new(Vec::new()));
    let scene = rotary_scene(vec![rotary_target(1, true, events.clone(), vec![1])]);

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(ScrollDispatchRenderer::new(scene), root_key, empty_content);
    shell.set_cursor(20.0, 30.0);
    events.borrow_mut().clear();

    assert!(!shell.rotary_scrolled(RotaryScrollEvent::new(0.0, 0.0, 1)));
    assert!(!shell.rotary_scrolled(RotaryScrollEvent::new(f32::NAN, 0.0, 1)));
    assert_eq!(events.borrow().len(), 0);
}

#[test]
fn window_level_rotary_handler_receives_unconsumed_events() {
    let _guard = test_guard();
    let events = Rc::new(RefCell::new(Vec::new()));
    let scene = rotary_scene(vec![rotary_target(1, false, events.clone(), vec![1])]);

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(ScrollDispatchRenderer::new(scene), root_key, empty_content);
    shell.set_cursor(20.0, 30.0);

    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    shell.set_on_rotary_scroll(move |event| {
        sink.borrow_mut().push(event);
        true
    });

    let rotary = RotaryScrollEvent::new(-40.0, 0.0, 77);
    assert!(shell.rotary_scrolled(rotary));
    assert_eq!(*seen.borrow(), vec![rotary]);
}

#[test]
fn window_level_rotary_handler_runs_with_no_hit_targets() {
    let _guard = test_guard();
    let scene = rotary_scene(Vec::new());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(ScrollDispatchRenderer::new(scene), root_key, empty_content);

    let seen = Rc::new(Cell::new(0.0f32));
    let sink = Rc::clone(&seen);
    shell.set_on_rotary_scroll(move |event| {
        sink.set(sink.get() + event.vertical_scroll_pixels);
        true
    });

    assert!(shell.rotary_scrolled(RotaryScrollEvent::new(-10.0, 0.0, 1)));
    assert!(shell.rotary_scrolled(RotaryScrollEvent::new(-5.0, 0.0, 2)));
    assert_eq!(seen.get(), -15.0);
}

#[test]
fn window_level_rotary_handler_is_skipped_when_a_modifier_consumed() {
    let _guard = test_guard();
    let events = Rc::new(RefCell::new(Vec::new()));
    let scene = rotary_scene(vec![rotary_target(1, true, events.clone(), vec![1])]);

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(ScrollDispatchRenderer::new(scene), root_key, empty_content);
    shell.set_cursor(20.0, 30.0);

    let calls = Rc::new(Cell::new(0));
    let counter = Rc::clone(&calls);
    shell.set_on_rotary_scroll(move |_| {
        counter.set(counter.get() + 1);
        true
    });

    assert!(shell.rotary_scrolled(RotaryScrollEvent::new(-10.0, 0.0, 1)));
    assert_eq!(
        calls.get(),
        0,
        "modifier consumption must take precedence over the window handler"
    );
}

#[test]
fn window_level_rotary_handler_can_decline() {
    let _guard = test_guard();
    let scene = rotary_scene(Vec::new());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(ScrollDispatchRenderer::new(scene), root_key, empty_content);

    shell.set_on_rotary_scroll(|_| false);
    assert!(!shell.rotary_scrolled(RotaryScrollEvent::new(-10.0, 0.0, 1)));

    shell.clear_on_rotary_scroll();
    assert!(!shell.rotary_scrolled(RotaryScrollEvent::new(-10.0, 0.0, 1)));
}

#[test]
fn rotary_detents_are_converted_with_the_scroll_factor() {
    let _guard = test_guard();
    let scene = rotary_scene(Vec::new());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(ScrollDispatchRenderer::new(scene), root_key, empty_content);

    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    shell.set_on_rotary_scroll(move |event| {
        sink.borrow_mut().push(event);
        true
    });

    shell.set_rotary_scroll_factor(10.0);
    assert_eq!(shell.rotary_scroll_factor(), 10.0);

    assert!(shell.rotary_scrolled_by_detents(1.0, 5));
    assert!(shell.rotary_scrolled_by_detents(-2.0, 6));

    let events = seen.borrow();
    assert_eq!(events[0].vertical_scroll_pixels, -10.0);
    assert_eq!(events[0].uptime_millis, 5);
    assert_eq!(events[1].vertical_scroll_pixels, 20.0);
}

#[test]
fn rotary_scroll_factor_rejects_unusable_values() {
    let _guard = test_guard();
    let scene = rotary_scene(Vec::new());
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(ScrollDispatchRenderer::new(scene), root_key, empty_content);

    shell.set_rotary_scroll_factor(12.0);
    shell.set_rotary_scroll_factor(0.0);
    shell.set_rotary_scroll_factor(-3.0);
    shell.set_rotary_scroll_factor(f32::NAN);

    assert_eq!(shell.rotary_scroll_factor(), 12.0);
}

#[test]
fn the_default_frame_rate_preference_boosts_on_interaction_and_holds_the_quiet_baseline() {
    use crate::FrameRatePreference;

    let auto = FrameRatePreference::default();
    assert_eq!(auto, FrameRatePreference::Auto);
    assert_eq!(auto.desired_rate_hz(true, true, Some(120.0)), 120.0);
    assert_eq!(
        auto.desired_rate_hz(true, false, Some(120.0)),
        FrameRatePreference::AUTO_QUIET_RATE_HZ,
        "an untouched animation votes the quiet baseline, not the panel max"
    );
    assert_eq!(
        auto.desired_rate_hz(false, true, Some(120.0)),
        120.0,
        "the boost holds through momentarily-still screens inside the hold-off, \
         so a gesture crossing a static screen does not flap the display rate"
    );
    assert_eq!(
        auto.desired_rate_hz(false, false, Some(120.0)),
        0.0,
        "an idle scene with no recent interaction clears the vote"
    );
    assert_eq!(
        auto.desired_rate_hz(true, true, None),
        FrameRatePreference::AUTO_QUIET_RATE_HZ
    );
    assert_eq!(
        auto.desired_rate_hz(true, true, Some(0.0)),
        FrameRatePreference::AUTO_QUIET_RATE_HZ
    );
}

#[test]
fn explicit_frame_rate_preferences_ignore_the_animation_state() {
    use crate::FrameRatePreference;

    assert_eq!(
        FrameRatePreference::NoPreference.desired_rate_hz(true, true, Some(120.0)),
        0.0
    );
    assert_eq!(
        FrameRatePreference::Exact(90.0).desired_rate_hz(false, false, Some(120.0)),
        90.0
    );
    assert_eq!(
        FrameRatePreference::Exact(-1.0).desired_rate_hz(true, true, Some(120.0)),
        0.0
    );
}

#[test]
fn the_shell_stores_the_frame_rate_preference_it_is_given() {
    let _guard = test_guard();
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, || {});

    assert_eq!(
        shell.frame_rate_preference(),
        crate::FrameRatePreference::Auto
    );
    shell.set_frame_rate_preference(crate::FrameRatePreference::Exact(60.0));
    assert_eq!(
        shell.frame_rate_preference(),
        crate::FrameRatePreference::Exact(60.0)
    );
}

thread_local! {
    static TRANSLATED_LAYER_POINTER_EVENTS: RefCell<Vec<(Point, Point)>> =
        const { RefCell::new(Vec::new()) };
    static UNTRANSLATED_PARENT_POINTER_EVENTS: RefCell<Vec<(Point, Point)>> =
        const { RefCell::new(Vec::new()) };
}

const TRANSLATED_LAYER_OFFSET: f32 = 60.0;

#[composable]
fn translated_layer_pointer_probe() {
    Box(
        Modifier::empty().fill_max_size().pointer_input(
            (),
            move |scope: PointerInputScope| async move {
                scope
                    .await_pointer_event_scope(|await_scope| async move {
                        loop {
                            let event = await_scope.await_pointer_event().await;
                            if event.kind == PointerEventKind::Down {
                                UNTRANSLATED_PARENT_POINTER_EVENTS.with(|slot| {
                                    slot.borrow_mut()
                                        .push((event.position, event.global_position));
                                });
                            }
                        }
                    })
                    .await;
            },
        ),
        BoxSpec::default(),
        || {
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .graphics_layer(|| GraphicsLayer {
                        translation_x: TRANSLATED_LAYER_OFFSET,
                        ..Default::default()
                    })
                    .pointer_input((), move |scope: PointerInputScope| async move {
                        scope
                            .await_pointer_event_scope(|await_scope| async move {
                                loop {
                                    let event = await_scope.await_pointer_event().await;
                                    if event.kind == PointerEventKind::Down {
                                        TRANSLATED_LAYER_POINTER_EVENTS.with(|slot| {
                                            slot.borrow_mut()
                                                .push((event.position, event.global_position));
                                        });
                                    }
                                }
                            })
                            .await;
                    }),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[test]
fn a_pointer_inside_a_translated_layer_reports_the_translation() {
    let _guard = test_guard();
    TRANSLATED_LAYER_POINTER_EVENTS.with(|slot| slot.borrow_mut().clear());
    UNTRANSLATED_PARENT_POINTER_EVENTS.with(|slot| slot.borrow_mut().clear());

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        root_key,
        translated_layer_pointer_probe,
    );
    shell.set_viewport(400.0, 400.0);
    shell.update();

    let tap_x = 200.0;
    assert!(
        shell.set_cursor(tap_x, 200.0),
        "the probe should be hovered"
    );
    shell.update();
    assert!(shell.pointer_pressed(), "pointer down should hit the probe");
    shell.update();

    let (local, global) = TRANSLATED_LAYER_POINTER_EVENTS
        .with(|slot| slot.borrow().first().copied())
        .expect("the translated node should have received the pointer down");
    assert!(
        (local.x - (tap_x - TRANSLATED_LAYER_OFFSET)).abs() < 0.5,
        "a node translated by {TRANSLATED_LAYER_OFFSET} must see the tap {TRANSLATED_LAYER_OFFSET} \
         to its left, got {local:?}"
    );
    assert!(
        (global.x - tap_x).abs() < 0.5,
        "global_position must be untouched by the layer, got {global:?}"
    );

    let (parent_local, parent_global) = UNTRANSLATED_PARENT_POINTER_EVENTS
        .with(|slot| slot.borrow().first().copied())
        .expect("the untranslated parent should have received the pointer down too");
    assert!(
        (parent_local.x - tap_x).abs() < 0.5 && (parent_global.x - tap_x).abs() < 0.5,
        "a handler outside the translated node sees the tap where it happened, got \
         {parent_local:?} / {parent_global:?}"
    );
}

thread_local! {
    static LIVE_RECORDER_LABEL: RefCell<String> = const { RefCell::new(String::new()) };
    static LIVE_RECORDER_REQUESTER: RefCell<Option<cranpose_ui::SemanticsRequester>> =
        const { RefCell::new(None) };
}

fn live_recorder_label() -> String {
    LIVE_RECORDER_LABEL.with(|label| label.borrow().clone())
}

fn set_live_recorder_label(value: &str) {
    LIVE_RECORDER_LABEL.with(|label| *label.borrow_mut() = value.to_string());
}

#[composable]
fn live_recorder_content() {
    let requester =
        cranpose_core::remember(cranpose_ui::SemanticsRequester::new).with(Clone::clone);
    LIVE_RECORDER_REQUESTER.with(|slot| *slot.borrow_mut() = Some(requester.clone()));
    Box(
        Modifier::empty()
            .size(Size {
                width: 100.0,
                height: 100.0,
            })
            .semantics_requester(&requester)
            .semantics(|config| {
                config.content_description = Some(live_recorder_label());
            }),
        BoxSpec::default(),
        || {},
    );
}

#[test]
fn a_live_semantics_recorder_is_stale_until_its_requester_says_otherwise() {
    let _guard = test_guard();
    set_live_recorder_label("boot");
    LIVE_RECORDER_REQUESTER.with(|slot| *slot.borrow_mut() = None);
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, live_recorder_content);
    shell.set_semantics_enabled(true);
    shell.process_frame();
    assert_eq!(
        shell.semantics_tree().map(semantics_tree_descriptions),
        Some(vec!["boot".to_string()]),
        "the first collection publishes what the recorder reported"
    );

    set_live_recorder_label("settings");
    for _ in 0..3 {
        shell.process_frame();
    }
    assert_eq!(
        shell.semantics_tree().map(semantics_tree_descriptions),
        Some(vec!["boot".to_string()]),
        "an unrequested change must not cost a re-collection every frame"
    );

    LIVE_RECORDER_REQUESTER.with(|slot| {
        slot.borrow()
            .as_ref()
            .expect("the requester is bound during composition")
            .invalidate()
    });
    shell.process_frame();
    assert_eq!(
        shell.semantics_tree().map(semantics_tree_descriptions),
        Some(vec!["settings".to_string()]),
        "invalidate() must republish without a recomposition or a layout pass"
    );
}

#[test]
fn a_pending_semantics_request_wakes_the_shell_without_dirtying_a_pixel() {
    let _guard = test_guard();
    set_live_recorder_label("boot");
    LIVE_RECORDER_REQUESTER.with(|slot| *slot.borrow_mut() = None);
    let root_key = location_key(file!(), line!(), column!());
    let mut shell = AppShell::new(TestRenderer::default(), root_key, live_recorder_content);
    shell.set_semantics_enabled(true);
    shell.process_frame();
    let _ = shell.semantics_tree();
    while shell.needs_update() {
        shell.update();
    }
    assert!(
        !shell.needs_redraw(),
        "the fixture must settle before the request is raised"
    );

    LIVE_RECORDER_REQUESTER.with(|slot| {
        slot.borrow()
            .as_ref()
            .expect("the requester is bound during composition")
            .invalidate()
    });
    assert!(
        shell.needs_update(),
        "a queued semantics request is work the UI thread owes"
    );
    assert!(
        !shell.needs_redraw(),
        "...but it changes no pixel, so it must not schedule a frame"
    );
}

#[test]
fn a_debug_report_describes_the_screen_and_says_so_when_there_is_none() {
    let _guard = test_guard();
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {},
    );

    shell.update();
    let report = shell.debug_info_report();
    assert!(
        report.contains("CURRENT SCREEN STATE"),
        "the report lost its header: {report}"
    );
    assert!(
        report.contains("LAYOUT TREE"),
        "the report described no layout: {report}"
    );

    assert_eq!(shell.log_debug_info(), shell.debug_info_report());
}

#[test]
fn semantics_are_off_until_a_bridge_asks_for_them() {
    let _guard = test_guard();
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {},
    );
    assert!(
        !shell.semantics_active(),
        "semantics tracking was on with no accessibility client attached"
    );
    shell.set_semantics_enabled(true);
    assert!(shell.semantics_active());
}

#[test]
fn the_slot_table_dump_describes_the_groups_the_composition_holds() {
    let _guard = test_guard();
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {},
    );
    shell.update();

    let groups = shell.debug_slot_table_groups();
    assert!(
        !groups.is_empty(),
        "a composed screen reported an empty slot table"
    );
    let entries = shell.debug_slot_entries().len();
    for (index, _key, _scope, length) in &groups {
        assert!(
            index + length <= entries,
            "group at {index} covers {length} slots, past the {entries} the table holds"
        );
    }
}

#[test]
fn subcompose_debug_readers_answer_none_for_a_node_that_is_not_one() {
    let _guard = test_guard();
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {},
    );
    shell.update();

    let live = shell.debug_live_subcompose_scope_ids();
    assert!(live.is_empty());

    let root = shell
        .layout_tree()
        .expect("layout tree available")
        .root()
        .node_id;
    assert!(shell.debug_subcompose_slot_table(root, 0).is_none());
    assert!(shell.debug_subcompose_slot_groups(root, 0).is_none());
}

#[test]
fn a_copy_with_no_focused_field_answers_none_and_syncs_nothing() {
    let _guard = test_guard();
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        || {},
    );
    shell.update();

    assert!(
        shell.on_copy().is_none(),
        "a copy with nothing focused produced text"
    );
    shell.sync_selection_to_primary();

    assert!(
        shell.ime_caret_geometry().is_none(),
        "a caret was reported with no focused text field"
    );
}
