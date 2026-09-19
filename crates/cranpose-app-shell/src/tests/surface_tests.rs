use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_render_common::RenderScene;
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, DragAndDropSource, DragAndDropTarget, LayoutBox, LayoutTree,
    Modifier, Point, Size, WindowRootDescriptor,
};
use cranpose_ui_graphics::{GraphicsLayer, PointerIcon};

use super::{
    tests::{
        HitGraphRenderer, ScopedUpdateCountingRenderer, SoftKeyboardProbe, TextFieldDispatchProbe,
        test_guard,
    },
    *,
};

fn window_root_at<R>(shell: &AppShell<R>, index: usize) -> u64
where
    R: Renderer,
    R::Error: std::fmt::Debug,
{
    shell
        .window_roots()
        .get(index)
        .map(|entry| entry.node as u64)
        .expect("a window root attached")
}

fn attach_first_window<R>(shell: &mut AppShell<R>, renderer: R) -> u64
where
    R: Renderer,
    R::Error: std::fmt::Debug,
{
    shell.update();
    let window_id = window_root_at(shell, 0);
    shell.add_window_surface(window_id, renderer, (200, 100), (200.0, 100.0));
    shell.update();
    window_id
}

#[test]
fn the_primary_has_content_only_outside_window_roots() {
    let _guard = test_guard();
    let torn: Rc<RefCell<Option<MutableState<bool>>>> = Rc::new(RefCell::new(None));
    let window = test_window(200.0, 100.0);
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        {
            let torn = Rc::clone(&torn);
            let window = Rc::clone(&window);
            move || {
                let is_torn = rememberMutableStateOf(|| true);
                *torn.borrow_mut() = Some(is_torn);
                let window = Rc::clone(&window);
                Column(Modifier::empty(), ColumnSpec::default(), move || {
                    Box(Modifier::empty(), BoxSpec::default(), || {});
                    let modifier = if is_torn.get() {
                        Modifier::empty().window_root(Rc::clone(&window))
                    } else {
                        Modifier::empty()
                    };
                    Box(modifier, BoxSpec::default(), || {
                        Box(
                            Modifier::empty().size(Size::new(90.0, 30.0)),
                            BoxSpec::default(),
                            || {},
                        );
                    });
                });
            }
        },
    );
    shell.update();
    assert!(
        !shell.primary_has_content(),
        "a root whose only sized node is in a window shows nothing of its own"
    );
    (*torn.borrow()).expect("state").set(false);
    shell.update();
    assert!(
        shell.primary_has_content(),
        "the page back inline is the primary's own content"
    );
}

type TransferLog = RefCell<Vec<String>>;

fn note(log: &TransferLog, line: impl Into<String>) {
    log.borrow_mut().push(line.into());
}

fn payload_number(payload: &cranpose_ui::DragAndDropPayload) -> u64 {
    payload.downcast_ref::<u64>().copied().unwrap_or(0)
}

fn logged_source(log: &Rc<TransferLog>) -> DragAndDropSource {
    let started = Rc::clone(log);
    let ended = Rc::clone(log);
    DragAndDropSource::new(7u64)
        .on_started(move |_| note(&started, "started"))
        .on_ended(move |outcome| note(&ended, format!("ended {outcome:?}")))
}

fn logged_target(log: &Rc<TransferLog>) -> DragAndDropTarget {
    let entered = Rc::clone(log);
    let exited = Rc::clone(log);
    let dropped = Rc::clone(log);
    DragAndDropTarget::new()
        .on_entered(move |payload| note(&entered, format!("entered {}", payload_number(payload))))
        .on_exited(move |_| note(&exited, "exited"))
        .on_drop(move |payload, at| {
            note(
                &dropped,
                format!("drop {} at {},{}", payload_number(payload), at.x, at.y),
            );
            true
        })
}

fn drag_and_drop_shell(log: &Rc<TransferLog>) -> AppShell<HitGraphRenderer> {
    let window = test_window(200.0, 100.0);
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        {
            let log = Rc::clone(log);
            move || {
                let log = Rc::clone(&log);
                let window = Rc::clone(&window);
                Column(Modifier::empty(), ColumnSpec::default(), move || {
                    Box(
                        Modifier::empty()
                            .size(Size::new(40.0, 40.0))
                            .drag_and_drop_source(logged_source(&log)),
                        BoxSpec::default(),
                        || {},
                    );
                    let target = logged_target(&log);
                    Box(
                        Modifier::empty().window_root(Rc::clone(&window)),
                        BoxSpec::default(),
                        move || {
                            Box(
                                Modifier::empty()
                                    .size(Size::new(90.0, 30.0))
                                    .drag_and_drop_target(target.clone()),
                                BoxSpec::default(),
                                || {},
                            );
                        },
                    );
                });
            }
        },
    );
    let window_id = attach_first_window(&mut shell, HitGraphRenderer::default());
    shell.set_screen_origin(Some(Point::new(0.0, 0.0)));
    shell
        .surface(RootId::Window(window_id))
        .expect("window surface")
        .set_screen_origin(Some(Point::new(300.0, 100.0)));
    shell
}

fn drag_from_the_source_to(shell: &mut AppShell<HitGraphRenderer>, x: f32, y: f32) {
    let mut primary = shell.primary();
    primary.set_cursor(10.0, 10.0);
    primary.pointer_pressed();
    primary.set_cursor(30.0, 10.0);
    primary.set_cursor(x, y);
}

#[test]
fn a_payload_dragged_out_of_one_window_drops_on_a_target_in_another() {
    let _guard = test_guard();
    let log = Rc::new(TransferLog::default());
    let mut shell = drag_and_drop_shell(&log);
    drag_from_the_source_to(&mut shell, 320.0, 115.0);
    assert_eq!(
        log.take(),
        vec!["started", "entered 7"],
        "the transfer reaches the target through the window's screen position"
    );
    shell.primary().set_cursor(500.0, 500.0);
    assert_eq!(log.take(), vec!["exited"]);
    shell.primary().set_cursor(320.0, 115.0);
    shell.primary().pointer_released();
    assert_eq!(
        log.take(),
        vec!["entered 7", "drop 7 at 20,15", "ended Dropped"],
        "the drop lands in the target's own coordinates"
    );
}

#[test]
fn a_payload_released_over_no_target_misses() {
    let _guard = test_guard();
    let log = Rc::new(TransferLog::default());
    let mut shell = drag_and_drop_shell(&log);
    drag_from_the_source_to(&mut shell, 500.0, 500.0);
    shell.primary().pointer_released();
    assert_eq!(log.take(), vec!["started", "ended Missed"]);
}

struct TestWindow {
    size: Cell<Size>,
}

impl WindowRootDescriptor for TestWindow {
    fn layout_size(&self) -> Size {
        self.size.get()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn test_window(width: f32, height: f32) -> Rc<dyn WindowRootDescriptor> {
    Rc::new(TestWindow {
        size: Cell::new(Size::new(width, height)),
    })
}

#[derive(Clone)]
struct TwoWindows {
    primary_presses: Rc<Cell<u32>>,
    window_presses: Rc<Cell<u32>>,
    window: Rc<dyn WindowRootDescriptor>,
    show_window: Rc<RefCell<Option<MutableState<bool>>>>,
}

impl TwoWindows {
    fn new() -> Self {
        Self {
            primary_presses: Rc::new(Cell::new(0)),
            window_presses: Rc::new(Cell::new(0)),
            window: test_window(200.0, 100.0),
            show_window: Rc::new(RefCell::new(None)),
        }
    }

    fn presses(&self) -> (u32, u32) {
        (self.primary_presses.get(), self.window_presses.get())
    }

    fn show_window(&self, shown: bool) {
        self.show_window
            .borrow()
            .as_ref()
            .expect("the content ran once")
            .set(shown);
    }

    fn content(&self) {
        let shown = rememberMutableStateOf(|| true);
        *self.show_window.borrow_mut() = Some(shown);
        let windows = self.clone();
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            let presses = Rc::clone(&windows.primary_presses);
            Box(
                Modifier::empty()
                    .size(Size::new(80.0, 40.0))
                    .pointer_icon(PointerIcon::POINTER)
                    .clickable(move |_| presses.set(presses.get() + 1)),
                BoxSpec::default(),
                || {},
            );
            if shown.get() {
                let presses = Rc::clone(&windows.window_presses);
                Box(
                    Modifier::empty().window_root(Rc::clone(&windows.window)),
                    BoxSpec::default(),
                    move || {
                        let presses = Rc::clone(&presses);
                        Box(
                            Modifier::empty()
                                .size(Size::new(90.0, 30.0))
                                .pointer_icon(PointerIcon::TEXT)
                                .clickable(move |_| presses.set(presses.get() + 1)),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            }
        });
    }
}

fn two_window_shell(windows: &TwoWindows) -> (AppShell<HitGraphRenderer>, u64) {
    let content = windows.clone();
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || content.content(),
    );
    let window_id = attach_first_window(&mut shell, HitGraphRenderer::default());
    (shell, window_id)
}

fn click<R>(surface: &mut SurfaceMut<'_, R>, x: f32, y: f32) -> bool
where
    R: Renderer,
    R::Error: std::fmt::Debug,
{
    surface.set_cursor(x, y);
    let pressed = surface.pointer_pressed();
    surface.pointer_released();
    pressed
}

fn count_boxes_sized(layout: &LayoutBox, width: f32, height: f32) -> usize {
    let own = usize::from(layout.rect.width == width && layout.rect.height == height);
    own + layout
        .children
        .iter()
        .map(|child| count_boxes_sized(child, width, height))
        .sum::<usize>()
}

fn root_size(tree: Option<&LayoutTree>) -> (f32, f32) {
    let root = tree.expect("a layout snapshot").root();
    (root.rect.width, root.rect.height)
}

#[test]
fn a_press_in_a_window_reaches_only_that_windows_content() {
    let _guard = test_guard();
    let windows = TwoWindows::new();
    let (mut shell, window_id) = two_window_shell(&windows);
    assert_eq!(shell.window_roots().len(), 1, "the window root registered");
    assert_eq!(
        shell.surface_ids(),
        vec![RootId::Primary, RootId::Window(window_id)]
    );

    let mut window = shell
        .surface(RootId::Window(window_id))
        .expect("window surface");
    assert!(
        click(&mut window, 40.0, 20.0),
        "the window's box is under the press"
    );
    assert_eq!(windows.presses(), (0, 1));
    assert_eq!(shell.active_root(), RootId::Window(window_id));

    let mut primary = shell.primary();
    assert!(click(&mut primary, 40.0, 20.0));
    assert_eq!(windows.presses(), (1, 1));
    assert_eq!(shell.active_root(), RootId::Primary);
}

#[test]
fn hovering_a_window_changes_only_that_windows_pointer_icon() {
    let _guard = test_guard();
    let windows = TwoWindows::new();
    let (mut shell, window_id) = two_window_shell(&windows);

    let mut window = shell
        .surface(RootId::Window(window_id))
        .expect("window surface");
    assert!(window.set_cursor(10.0, 10.0));
    assert_eq!(window.take_pointer_icon_change(), Some(PointerIcon::TEXT));
    assert_eq!(
        shell.take_pointer_icon_change(),
        None,
        "the primary window's pointer did not move"
    );

    assert!(shell.set_cursor(10.0, 10.0));
    assert_eq!(shell.take_pointer_icon_change(), Some(PointerIcon::POINTER));
    let window = shell
        .surface(RootId::Window(window_id))
        .expect("window surface");
    assert_eq!(window.take_pointer_icon_change(), None);
}

#[test]
fn each_surface_snapshots_the_layout_of_its_own_root() {
    let _guard = test_guard();
    let windows = TwoWindows::new();
    let (mut shell, window_id) = two_window_shell(&windows);

    shell.with_layout_tree(|tree| {
        let root = tree.expect("primary layout").root();
        assert_eq!(root_size(tree), (800.0, 600.0));
        assert_eq!(
            count_boxes_sized(root, 90.0, 30.0),
            0,
            "the window's box is not here"
        );
        assert_eq!(
            count_boxes_sized(root, 200.0, 100.0),
            0,
            "nor is the window root"
        );
    });
    let mut window = shell
        .surface(RootId::Window(window_id))
        .expect("window surface");
    window.with_layout_tree(|tree| {
        let root = tree.expect("window layout").root();
        assert_eq!(root_size(tree), (200.0, 100.0));
        assert_eq!(count_boxes_sized(root, 90.0, 30.0), 1);
        assert_eq!(
            count_boxes_sized(root, 80.0, 40.0),
            0,
            "the primary's box is not here"
        );
    });
}

#[test]
fn a_window_surface_follows_its_root_out_of_the_tree_and_back() {
    let _guard = test_guard();
    let windows = TwoWindows::new();
    let (mut shell, window_id) = two_window_shell(&windows);
    assert!(
        shell
            .surface(RootId::Window(window_id))
            .and_then(|surface| surface.root())
            .is_some(),
        "the surface found its root"
    );

    windows.show_window(false);
    shell.update();
    assert!(shell.window_roots().is_empty());
    let window = shell
        .surface(RootId::Window(window_id))
        .expect("the surface stays");
    assert_eq!(window.root(), None);
    assert!(
        window.scene().hit_test(40.0, 20.0).is_empty(),
        "a surface whose root left draws nothing"
    );

    windows.show_window(true);
    shell.update();
    let mut window = shell
        .surface(RootId::Window(window_id))
        .expect("window surface");
    assert!(window.root().is_some(), "the window root came back");
    assert!(click(&mut window, 40.0, 20.0));
    assert_eq!(windows.presses(), (0, 1));
}

#[test]
fn removing_a_window_surface_hands_back_its_renderer() {
    let _guard = test_guard();
    let windows = TwoWindows::new();
    let (mut shell, window_id) = two_window_shell(&windows);

    assert!(shell.remove_window_surface(window_id).is_some());
    assert!(shell.surface(RootId::Window(window_id)).is_none());
    assert!(shell.remove_window_surface(window_id).is_none());
    shell.update();
    assert_eq!(
        shell.window_roots().len(),
        1,
        "the window root stays in the tree until the app removes it"
    );
    assert!(
        shell.set_cursor(40.0, 20.0),
        "the primary still draws its own content"
    );
}

#[test]
fn the_soft_keyboard_belongs_to_the_active_surface() {
    let _guard = test_guard();
    let windows = TwoWindows::new();
    let (mut shell, window_id) = two_window_shell(&windows);
    let primary_keyboard = Rc::new(SoftKeyboardProbe::default());
    let window_keyboard = Rc::new(SoftKeyboardProbe::default());
    shell.set_platform_text_input(primary_keyboard.clone());
    shell
        .surface(RootId::Window(window_id))
        .expect("window surface")
        .set_platform_text_input(window_keyboard.clone());

    let mut window = shell
        .surface(RootId::Window(window_id))
        .expect("window surface");
    assert!(click(&mut window, 40.0, 20.0));
    let focus_flag = Rc::new(RefCell::new(false));
    let handler = Rc::new(TextFieldDispatchProbe::default());
    shell.debug_enter_app_context(|| {
        cranpose_ui::text_field_focus::request_focus(Rc::clone(&focus_flag), handler.clone(), 0);
    });
    assert_eq!(*window_keyboard.calls.borrow(), vec!["show"]);
    assert!(primary_keyboard.calls.borrow().is_empty());

    shell.set_active_root(RootId::Primary);
    shell.debug_enter_app_context(cranpose_ui::text_field_focus::clear_focus);
    assert_eq!(
        *window_keyboard.calls.borrow(),
        vec!["show", "hide"],
        "the hide reaches the window that showed the keyboard"
    );
    assert!(primary_keyboard.calls.borrow().is_empty());
}

#[derive(Clone, Default)]
struct RendererCounts {
    rebuilds: Rc<Cell<usize>>,
    updates: Rc<Cell<usize>>,
    visual_updates: Rc<Cell<usize>>,
    last_dirty_nodes: Rc<RefCell<Vec<NodeId>>>,
}

impl RendererCounts {
    fn renderer(&self) -> ScopedUpdateCountingRenderer {
        ScopedUpdateCountingRenderer::with_visual_updates(
            Rc::clone(&self.rebuilds),
            Rc::clone(&self.updates),
            Rc::clone(&self.visual_updates),
            Rc::clone(&self.last_dirty_nodes),
        )
    }

    fn reset(&self) {
        self.rebuilds.set(0);
        self.updates.set(0);
        self.visual_updates.set(0);
        self.last_dirty_nodes.borrow_mut().clear();
    }

    fn scene_work(&self) -> (usize, usize, usize) {
        (
            self.rebuilds.get(),
            self.updates.get(),
            self.visual_updates.get(),
        )
    }
}

fn translated_box(offset: MutableState<f32>) {
    Box(
        Modifier::empty()
            .size(Size::new(120.0, 40.0))
            .graphics_layer(move || GraphicsLayer {
                translation_x: offset.get(),
                ..GraphicsLayer::default()
            }),
        BoxSpec::default(),
        || {},
    );
}

#[test]
fn a_draw_change_in_a_window_updates_only_that_windows_scene() {
    let _guard = test_guard();
    let primary = RendererCounts::default();
    let window_counts = RendererCounts::default();
    let window = test_window(200.0, 100.0);
    let offsets: Rc<RefCell<Option<(MutableState<f32>, MutableState<f32>)>>> =
        Rc::new(RefCell::new(None));
    let mut shell = AppShell::new(
        primary.renderer(),
        location_key(file!(), line!(), column!()),
        {
            let offsets = Rc::clone(&offsets);
            let window = Rc::clone(&window);
            move || {
                let primary_offset = rememberMutableStateOf(|| 0.0f32);
                let window_offset = rememberMutableStateOf(|| 0.0f32);
                *offsets.borrow_mut() = Some((primary_offset, window_offset));
                let window = Rc::clone(&window);
                Column(Modifier::empty(), ColumnSpec::default(), move || {
                    translated_box(primary_offset);
                    Box(
                        Modifier::empty().window_root(Rc::clone(&window)),
                        BoxSpec::default(),
                        move || translated_box(window_offset),
                    );
                });
            }
        },
    );
    let window_id = attach_first_window(&mut shell, window_counts.renderer());
    primary.reset();
    window_counts.reset();
    shell.debug_enter_app_context(cranpose_ui::request_render_invalidation);
    shell.update();
    assert_eq!(
        (primary.scene_work(), window_counts.scene_work()),
        ((0, 0, 1), (0, 0, 1)),
        "a bare render invalidation lets each surface take its own nodes' redraw flags"
    );
    let primary_nodes = primary.last_dirty_nodes.borrow().clone();
    assert!(
        !window_counts
            .last_dirty_nodes
            .borrow()
            .iter()
            .any(|node| primary_nodes.contains(node)),
        "the window's nodes are not the primary's"
    );
    let (primary_offset, window_offset) = (*offsets.borrow()).expect("offsets");
    primary.reset();
    window_counts.reset();

    window_offset.set(24.0);
    let result = shell.update();

    assert!(result.visual_changed);
    assert_eq!(
        primary.scene_work(),
        (0, 0, 0),
        "the primary scene is untouched"
    );
    assert_eq!(
        window_counts.scene_work(),
        (0, 0, 1),
        "the window scene took the visual update"
    );
    assert!(!window_counts.last_dirty_nodes.borrow().is_empty());
    assert!(
        shell
            .surface(RootId::Window(window_id))
            .expect("window surface")
            .last_update_result()
            .visual_changed
    );
    assert!(
        !shell.primary().last_update_result().visual_changed,
        "only the window owes the display a frame"
    );

    primary.reset();
    window_counts.reset();
    primary_offset.set(24.0);
    shell.update();
    assert_eq!(primary.scene_work(), (0, 0, 1));
    assert_eq!(window_counts.scene_work(), (0, 0, 0));
}

#[test]
fn a_surface_keeps_owing_its_frame_until_the_platform_takes_it() {
    let _guard = test_guard();
    let primary = RendererCounts::default();
    let window_counts = RendererCounts::default();
    let window = test_window(200.0, 100.0);
    let offset: Rc<RefCell<Option<MutableState<f32>>>> = Rc::new(RefCell::new(None));
    let mut shell = AppShell::new(
        primary.renderer(),
        location_key(file!(), line!(), column!()),
        {
            let offset = Rc::clone(&offset);
            let window = Rc::clone(&window);
            move || {
                let window_offset = rememberMutableStateOf(|| 0.0f32);
                *offset.borrow_mut() = Some(window_offset);
                let window = Rc::clone(&window);
                Box(
                    Modifier::empty().window_root(Rc::clone(&window)),
                    BoxSpec::default(),
                    move || translated_box(window_offset),
                );
            }
        },
    );
    let window_id = attach_first_window(&mut shell, window_counts.renderer());
    shell.update();
    assert!(shell.take_frame_owed(), "the first frames drew the primary");
    assert!(
        shell
            .surface(RootId::Window(window_id))
            .expect("window surface")
            .take_frame_owed()
    );

    let offset = (*offset.borrow()).expect("offset");
    offset.set(12.0);
    shell.update();
    shell.update();
    let mut window = shell
        .surface(RootId::Window(window_id))
        .expect("window surface");
    assert!(
        !window.last_update_result().visual_changed,
        "the second update drew nothing new"
    );
    assert!(
        window.frame_owed(),
        "the frame the first update drew is still owed to the platform"
    );
    assert!(window.take_frame_owed());
    assert!(!window.take_frame_owed(), "taken once");
    assert!(!shell.take_frame_owed(), "the primary drew nothing");
}

fn press_recorder(recorded: Rc<Cell<Option<Option<cranpose_ui::Point>>>>) {
    Box(
        Modifier::empty().size(Size::new(80.0, 40.0)).pointer_input(
            (),
            move |scope: cranpose_ui::PointerInputScope| {
                let recorded = Rc::clone(&recorded);
                async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                if event.kind == cranpose_ui::PointerEventKind::Down {
                                    recorded.set(Some(event.screen_position));
                                }
                            }
                        })
                        .await;
                }
            },
        ),
        BoxSpec::default(),
        || {},
    );
}

#[test]
fn a_press_carries_the_screen_position_of_its_surfaces_window() {
    let _guard = test_guard();
    let primary_press = Rc::new(Cell::new(None));
    let window_press = Rc::new(Cell::new(None));
    let window = test_window(200.0, 100.0);
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        {
            let primary_press = Rc::clone(&primary_press);
            let window_press = Rc::clone(&window_press);
            let window = Rc::clone(&window);
            move || {
                let primary_press = Rc::clone(&primary_press);
                let window_press = Rc::clone(&window_press);
                let window = Rc::clone(&window);
                Column(Modifier::empty(), ColumnSpec::default(), move || {
                    press_recorder(Rc::clone(&primary_press));
                    let window_press = Rc::clone(&window_press);
                    Box(
                        Modifier::empty().window_root(Rc::clone(&window)),
                        BoxSpec::default(),
                        move || press_recorder(Rc::clone(&window_press)),
                    );
                });
            }
        },
    );
    let window_id = attach_first_window(&mut shell, HitGraphRenderer::default());

    let mut window = shell
        .surface(RootId::Window(window_id))
        .expect("window surface");
    window.set_screen_origin(Some(cranpose_ui::Point::new(100.0, 50.0)));
    assert!(click(&mut window, 10.0, 5.0));
    assert_eq!(
        window_press.get(),
        Some(Some(cranpose_ui::Point::new(110.0, 55.0))),
        "the window's origin plus the press inside it"
    );

    window.set_screen_origin(Some(cranpose_ui::Point::new(0.0, 300.0)));
    assert!(click(&mut window, 10.0, 5.0));
    assert_eq!(
        window_press.get(),
        Some(Some(cranpose_ui::Point::new(10.0, 305.0))),
        "the window moved before the next press"
    );

    let mut primary = shell.primary();
    assert!(click(&mut primary, 10.0, 5.0));
    assert_eq!(
        primary_press.get(),
        Some(None),
        "the primary window's position was never told"
    );
}

fn find_box_sized(layout: &LayoutBox, width: f32, height: f32) -> Option<(f32, f32)> {
    if layout.rect.width == width && layout.rect.height == height {
        return Some((layout.rect.x, layout.rect.y));
    }
    layout
        .children
        .iter()
        .find_map(|child| find_box_sized(child, width, height))
}

#[test]
fn movable_content_torn_into_a_fresh_column_lays_out_after_the_strip() {
    let _guard = test_guard();
    let torn: Rc<RefCell<Option<MutableState<bool>>>> = Rc::new(RefCell::new(None));
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        {
            let torn = Rc::clone(&torn);
            move || {
                let is_torn = rememberMutableStateOf(|| false);
                *torn.borrow_mut() = Some(is_torn);
                Column(Modifier::empty(), ColumnSpec::default(), move || {
                    if !is_torn.get() {
                        Column(Modifier::empty(), ColumnSpec::default(), || {
                            cranpose_core::movable("page", || {
                                Box(
                                    Modifier::empty().size(Size::new(90.0, 30.0)),
                                    BoxSpec::default(),
                                    || {},
                                );
                            });
                        });
                    } else {
                        Column(Modifier::empty(), ColumnSpec::default(), || {
                            Box(
                                Modifier::empty().size(Size::new(80.0, 36.0)),
                                BoxSpec::default(),
                                || {},
                            );
                            cranpose_core::movable("page", || {
                                Box(
                                    Modifier::empty().size(Size::new(90.0, 30.0)),
                                    BoxSpec::default(),
                                    || {},
                                );
                            });
                        });
                    }
                });
            }
        },
    );
    shell.update();
    (*torn.borrow()).expect("state").set(true);
    shell.update();
    shell.update();
    let page =
        shell.with_layout_tree(|tree| find_box_sized(tree.expect("layout").root(), 90.0, 30.0));
    assert_eq!(
        page,
        Some((0.0, 36.0)),
        "the page lays out below the 36px strip"
    );
}

#[cranpose_ui::composable]
#[allow(non_snake_case)]
fn TornStrip() {
    Box(
        Modifier::empty().size(Size::new(80.0, 36.0)),
        BoxSpec::default(),
        || {},
    );
}

#[cranpose_ui::composable]
#[allow(non_snake_case)]
fn TornBody(page: u64) {
    let _clicks = rememberMutableStateOf(|| 0u32);
    let _ = page;
    Box(
        Modifier::empty().size(Size::new(90.0, 30.0)),
        BoxSpec::default(),
        || {},
    );
}

#[cranpose_ui::composable]
#[allow(non_snake_case)]
fn TornWindowChrome(page: u64) {
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        TornStrip();
        cranpose_core::movable(("page", page), move || TornBody(page));
    });
}

#[test]
fn a_page_torn_into_a_new_window_root_lays_out_below_that_windows_strip() {
    let _guard = test_guard();
    let torn: Rc<RefCell<Option<MutableState<bool>>>> = Rc::new(RefCell::new(None));
    let ticks: Rc<RefCell<Option<MutableState<u32>>>> = Rc::new(RefCell::new(None));
    let first = test_window(200.0, 100.0);
    let second = test_window(200.0, 100.0);
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        {
            let torn = Rc::clone(&torn);
            let ticks = Rc::clone(&ticks);
            let first = Rc::clone(&first);
            let second = Rc::clone(&second);
            move || {
                let is_torn = rememberMutableStateOf(|| false);
                *torn.borrow_mut() = Some(is_torn);
                let tick = rememberMutableStateOf(|| 0u32);
                *ticks.borrow_mut() = Some(tick);
                let first = Rc::clone(&first);
                let second = Rc::clone(&second);
                Column(Modifier::empty(), ColumnSpec::default(), move || {
                    let shown_first = if is_torn.get() { 2 } else { 1 };
                    Box(
                        Modifier::empty().window_root(Rc::clone(&first)),
                        BoxSpec::default(),
                        move || {
                            cranpose_ui::widgets::PopupHost(move || TornWindowChrome(shown_first))
                        },
                    );
                    if is_torn.get() {
                        Box(
                            Modifier::empty().window_root(Rc::clone(&second)),
                            BoxSpec::default(),
                            move || {
                                tick.get();
                                cranpose_ui::widgets::PopupHost(move || TornWindowChrome(1));
                            },
                        );
                    }
                });
            }
        },
    );
    attach_first_window(&mut shell, HitGraphRenderer::default());
    (*torn.borrow()).expect("state").set(true);
    shell.update();
    let second_window_id = window_root_at(&shell, 1);
    shell.add_window_surface(
        second_window_id,
        HitGraphRenderer::default(),
        (200, 100),
        (200.0, 100.0),
    );
    shell.update();
    shell.update();
    let page_in_second = |shell: &mut AppShell<HitGraphRenderer>| {
        shell
            .surface(RootId::Window(second_window_id))
            .expect("second window")
            .with_layout_tree(|tree| find_box_sized(tree.expect("layout").root(), 90.0, 30.0))
    };
    assert_eq!(
        page_in_second(&mut shell),
        Some((0.0, 36.0)),
        "the page lays out below the new window's strip"
    );

    (*ticks.borrow()).expect("ticks").set(1);
    shell.update();
    shell.update();
    assert_eq!(
        page_in_second(&mut shell),
        Some((0.0, 36.0)),
        "a recomposition that skips the strip and the page leaves the page in place"
    );
}
