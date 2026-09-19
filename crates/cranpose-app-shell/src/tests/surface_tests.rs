use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_render_common::RenderScene;
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, LayoutBox, LayoutTree, Modifier, Size, WindowRootDescriptor,
};
use cranpose_ui_graphics::{GraphicsLayer, PointerIcon};

use super::{
    tests::{
        HitGraphRenderer, ScopedUpdateCountingRenderer, SoftKeyboardProbe, TextFieldDispatchProbe,
        test_guard,
    },
    *,
};

const WINDOW: u64 = 7;

/// A window whose size a test changes between passes.
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

/// What the two-window content lets a test observe and steer.
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

    /// A column holding a clickable box and, while shown, a window root
    /// whose content is a clickable box of its own.
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
                    Modifier::empty().window_root(WINDOW, Rc::clone(&windows.window)),
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

fn two_window_shell(windows: &TwoWindows) -> AppShell<HitGraphRenderer> {
    let content = windows.clone();
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || content.content(),
    );
    shell.add_window_surface(
        WINDOW,
        HitGraphRenderer::default(),
        (200, 100),
        (200.0, 100.0),
    );
    shell.update();
    shell
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
    let mut shell = two_window_shell(&windows);
    assert_eq!(shell.window_roots().len(), 1, "the window root registered");
    assert_eq!(
        shell.surface_ids(),
        vec![RootId::Primary, RootId::Window(WINDOW)]
    );

    let mut window = shell
        .surface(RootId::Window(WINDOW))
        .expect("window surface");
    assert!(
        click(&mut window, 40.0, 20.0),
        "the window's box is under the press"
    );
    assert_eq!(windows.presses(), (0, 1));
    assert_eq!(shell.active_root(), RootId::Window(WINDOW));

    let mut primary = shell.primary();
    assert!(click(&mut primary, 40.0, 20.0));
    assert_eq!(windows.presses(), (1, 1));
    assert_eq!(shell.active_root(), RootId::Primary);
}

#[test]
fn hovering_a_window_changes_only_that_windows_pointer_icon() {
    let _guard = test_guard();
    let windows = TwoWindows::new();
    let mut shell = two_window_shell(&windows);

    let mut window = shell
        .surface(RootId::Window(WINDOW))
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
        .surface(RootId::Window(WINDOW))
        .expect("window surface");
    assert_eq!(window.take_pointer_icon_change(), None);
}

#[test]
fn each_surface_snapshots_the_layout_of_its_own_root() {
    let _guard = test_guard();
    let windows = TwoWindows::new();
    let mut shell = two_window_shell(&windows);

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
        .surface(RootId::Window(WINDOW))
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
    let mut shell = two_window_shell(&windows);
    assert!(
        shell
            .surface(RootId::Window(WINDOW))
            .and_then(|surface| surface.root())
            .is_some(),
        "the surface found its root"
    );

    windows.show_window(false);
    shell.update();
    assert!(shell.window_roots().is_empty());
    let window = shell
        .surface(RootId::Window(WINDOW))
        .expect("the surface stays");
    assert_eq!(window.root(), None);
    assert!(
        window.scene().hit_test(40.0, 20.0).is_empty(),
        "a surface whose root left draws nothing"
    );

    windows.show_window(true);
    shell.update();
    let mut window = shell
        .surface(RootId::Window(WINDOW))
        .expect("window surface");
    assert!(window.root().is_some(), "the window root came back");
    assert!(click(&mut window, 40.0, 20.0));
    assert_eq!(windows.presses(), (0, 1));
}

#[test]
fn removing_a_window_surface_hands_back_its_renderer() {
    let _guard = test_guard();
    let windows = TwoWindows::new();
    let mut shell = two_window_shell(&windows);

    assert!(shell.remove_window_surface(WINDOW).is_some());
    assert!(shell.surface(RootId::Window(WINDOW)).is_none());
    assert!(shell.remove_window_surface(WINDOW).is_none());
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
    let mut shell = two_window_shell(&windows);
    let primary_keyboard = Rc::new(SoftKeyboardProbe::default());
    let window_keyboard = Rc::new(SoftKeyboardProbe::default());
    shell.set_platform_text_input(primary_keyboard.clone());
    shell
        .surface(RootId::Window(WINDOW))
        .expect("window surface")
        .set_platform_text_input(window_keyboard.clone());

    let mut window = shell
        .surface(RootId::Window(WINDOW))
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

/// Counters of one scoped-update renderer.
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
                        Modifier::empty().window_root(WINDOW, Rc::clone(&window)),
                        BoxSpec::default(),
                        move || translated_box(window_offset),
                    );
                });
            }
        },
    );
    shell.add_window_surface(WINDOW, window_counts.renderer(), (200, 100), (200.0, 100.0));
    shell.update();
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
            .surface(RootId::Window(WINDOW))
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
                    Modifier::empty().window_root(WINDOW, Rc::clone(&window)),
                    BoxSpec::default(),
                    move || translated_box(window_offset),
                );
            }
        },
    );
    shell.add_window_surface(WINDOW, window_counts.renderer(), (200, 100), (200.0, 100.0));
    shell.update();
    shell.update();
    assert!(shell.take_frame_owed(), "the first frames drew the primary");
    assert!(
        shell
            .surface(RootId::Window(WINDOW))
            .expect("window surface")
            .take_frame_owed()
    );

    let offset = (*offset.borrow()).expect("offset");
    offset.set(12.0);
    shell.update();
    shell.update();
    let mut window = shell
        .surface(RootId::Window(WINDOW))
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
