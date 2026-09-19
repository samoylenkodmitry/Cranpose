use std::sync::Arc;

use super::*;

struct OwnedWindowState {
    _runtime: cranpose_core::Runtime,
    _position: cranpose_core::OwnedMutableState<Option<Point>>,
    _size: cranpose_core::OwnedMutableState<Size>,
    _presented: cranpose_core::OwnedMutableState<bool>,
    state: WindowState,
}

fn test_window_state(width: f32, height: f32) -> OwnedWindowState {
    let runtime = cranpose_core::Runtime::new(Arc::new(cranpose_core::DefaultScheduler));
    let handle = runtime.handle();
    let position = cranpose_core::OwnedMutableState::with_runtime(None::<Point>, handle.clone());
    let size =
        cranpose_core::OwnedMutableState::with_runtime(Size::new(width, height), handle.clone());
    let presented = cranpose_core::OwnedMutableState::with_runtime(false, handle);
    let state = WindowState {
        position: position.handle(),
        size: size.handle(),
        presented: presented.handle(),
    };
    OwnedWindowState {
        _runtime: runtime,
        _position: position,
        _size: size,
        _presented: presented,
        state,
    }
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn with_request_test_registry<R>(f: impl FnOnce(&Rc<NativeWindowRegistry>) -> R) -> R {
    let registry = Rc::new(NativeWindowRegistry::default());
    with_native_window_registry(&registry, || f(&registry))
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn reset_request_test_state(registry: &NativeWindowRegistry) {
    clear_native_window_requests(registry);
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn request_count(registry: &NativeWindowRegistry) -> usize {
    native_window_requests(registry).len()
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[composable]
#[allow(non_snake_case)]
fn WindowBox(config: WindowConfig) -> cranpose_core::NodeId {
    cranpose_ui::Box(
        Modifier::empty().window(config),
        cranpose_ui::BoxSpec::default(),
        || {},
    )
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn native_window_requests_are_isolated_by_registry() {
    let first_registry = Rc::new(NativeWindowRegistry::default());
    let second_registry = Rc::new(NativeWindowRegistry::default());
    let first_key = WindowId::from_static("first-registry-window");
    let second_key = WindowId::from_static("second-registry-window");
    let first_owner = Rc::new(());
    let second_owner = Rc::new(());
    let first_content = Rc::new(NativeWindowRoot::new(Size::new(1.0, 1.0)));
    let second_content = Rc::new(NativeWindowRoot::new(Size::new(1.0, 1.0)));

    with_native_window_registry(&first_registry, || {
        register_native_window(
            first_key,
            NativeWindowOptions::new("first", 80.0, 40.0),
            NativeWindowEvents::default(),
            None,
            None,
            first_content,
            first_owner,
        );
    });
    with_native_window_registry(&second_registry, || {
        register_native_window(
            second_key,
            NativeWindowOptions::new("second", 90.0, 45.0),
            NativeWindowEvents::default(),
            None,
            None,
            second_content,
            second_owner,
        );
    });

    let first_requests = native_window_requests(&first_registry);
    let second_requests = native_window_requests(&second_registry);

    assert_eq!(first_requests.len(), 1);
    assert_eq!(first_requests[0].key, first_key);
    assert_eq!(second_requests.len(), 1);
    assert_eq!(second_requests[0].key, second_key);
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
struct RequestTestComposition {
    _context: Rc<cranpose_ui::AppContext>,
    _scope: cranpose_ui::AppContextScope,
    runtime: cranpose_core::Runtime,
    composition: cranpose_core::Composition<cranpose_core::MemoryApplier>,
    registry: Rc<NativeWindowRegistry>,
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
impl RequestTestComposition {
    fn with_registry<R>(
        &mut self,
        f: impl FnOnce(&mut cranpose_core::Composition<cranpose_core::MemoryApplier>) -> R,
    ) -> R {
        let registry = Rc::clone(&self.registry);
        with_native_window_registry(&registry, || f(&mut self.composition))
    }
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn test_app_context_scope() -> (Rc<cranpose_ui::AppContext>, cranpose_ui::AppContextScope) {
    let context = cranpose_ui::AppContext::new();
    let scope = context.enter_scope();
    (context, scope)
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn request_test_composition() -> RequestTestComposition {
    let (context, scope) = test_app_context_scope();
    let runtime = cranpose_core::Runtime::new(Arc::new(cranpose_core::DefaultScheduler));
    let composition = cranpose_core::Composition::with_runtime(
        cranpose_core::MemoryApplier::new(),
        runtime.clone(),
    );
    RequestTestComposition {
        _context: context,
        _scope: scope,
        runtime,
        composition,
        registry: Rc::new(NativeWindowRegistry::default()),
    }
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[composable]
#[allow(non_snake_case)]
fn RequestCounterText(counter: cranpose_core::MutableState<i32>) {
    cranpose_ui::Text(
        format!("Counter {}", counter.get()),
        cranpose_ui::Modifier::empty(),
        cranpose_ui::TextStyle::default(),
    );
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[composable]
#[allow(non_snake_case)]
fn PersistentRequestRoot(counter: cranpose_core::MutableState<i32>) {
    RequestCounterText(counter);
    WindowBox(WindowConfig::new("Persistent request", 100.0, 50.0));
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[composable]
#[allow(non_snake_case)]
fn ConditionalRequestRoot(show: cranpose_core::MutableState<bool>) {
    if show.get() {
        WindowBox(WindowConfig::new("Conditional request", 100.0, 50.0));
    }
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[composable]
#[allow(non_snake_case)]
fn KeyedReplacementRequestRoot(show: cranpose_core::MutableState<bool>) {
    let active = show.get();
    cranpose_core::with_key(&active, || {
        if active {
            WindowBox(WindowConfig::new("Keyed replacement request", 100.0, 50.0));
        } else {
            cranpose_ui::Text(
                "Inactive branch",
                cranpose_ui::Modifier::empty(),
                cranpose_ui::TextStyle::default(),
            );
        }
    });
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn native_window_request_survives_unrelated_scoped_recompose() {
    let mut test = request_test_composition();
    reset_request_test_state(&test.registry);
    let counter = cranpose_core::MutableState::with_runtime(0i32, test.runtime.handle());
    let root_key = cranpose_core::location_key(file!(), line!(), column!());
    test.with_registry(|composition| {
        composition
            .render_stable(root_key, || PersistentRequestRoot(counter))
            .expect("initial persistent native-window request render");
    });
    assert_eq!(request_count(&test.registry), 1);

    counter.set(1);
    test.with_registry(|composition| {
        composition
            .reconcile(root_key, || PersistentRequestRoot(counter))
            .expect("persistent native-window request reconcile");
    });

    assert_eq!(
        request_count(&test.registry),
        1,
        "unchanged native-window declarations must stay registered when only a sibling scope recomposes"
    );
    clear_native_window_requests(&test.registry);
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn native_window_request_unregisters_when_conditional_declaration_is_removed() {
    let mut test = request_test_composition();
    reset_request_test_state(&test.registry);
    let show = cranpose_core::MutableState::with_runtime(true, test.runtime.handle());
    let root_key = cranpose_core::location_key(file!(), line!(), column!());
    test.with_registry(|composition| {
        composition
            .render_stable(root_key, || ConditionalRequestRoot(show))
            .expect("initial conditional native-window request render");
    });
    assert_eq!(request_count(&test.registry), 1);

    show.set(false);
    test.with_registry(|composition| {
        composition
            .reconcile(root_key, || ConditionalRequestRoot(show))
            .expect("conditional native-window request reconcile");
    });

    assert_eq!(
        request_count(&test.registry),
        0,
        "a window modifier that left the composition withdraws its request"
    );
    clear_native_window_requests(&test.registry);
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn native_window_request_unregisters_when_keyed_branch_is_replaced() {
    let mut test = request_test_composition();
    reset_request_test_state(&test.registry);
    let show = cranpose_core::MutableState::with_runtime(true, test.runtime.handle());
    let root_key = cranpose_core::location_key(file!(), line!(), column!());
    test.with_registry(|composition| {
        composition
            .render_stable(root_key, || KeyedReplacementRequestRoot(show))
            .expect("initial keyed native-window request render");
    });
    assert_eq!(request_count(&test.registry), 1);

    show.set(false);
    test.with_registry(|composition| {
        composition
            .reconcile(root_key, || KeyedReplacementRequestRoot(show))
            .expect("keyed native-window request reconcile");
    });

    assert_eq!(
        request_count(&test.registry),
        0,
        "keyed branch replacement must unregister native-window declarations from the inactive branch"
    );
    clear_native_window_requests(&test.registry);
}

#[test]
fn borderless_options_disable_decorations_and_resizing() {
    let options = NativeWindowOptions::borderless("Tool", 100.0, 50.0);
    assert_eq!(options.title, "Tool");
    assert_eq!(options.width, 100.0);
    assert_eq!(options.height, 50.0);
    assert_eq!(options.position_origin, NativeWindowPositionOrigin::Screen);
    assert!(!options.decorations);
    assert!(!options.resizable);
    assert!(options.visible);
}

#[test]
fn option_builders_update_specific_fields() {
    let options = NativeWindowOptions::new("Panel", 10.0, 20.0)
        .with_position(3.0, 4.0)
        .with_transparent(true)
        .with_resizable(false)
        .with_visible(false)
        .with_always_on_top(true)
        .with_min_size(5.0, 6.0)
        .with_max_size(50.0, 60.0);
    assert_eq!(options.x, Some(3.0));
    assert_eq!(options.y, Some(4.0));
    assert_eq!(options.position_origin, NativeWindowPositionOrigin::Screen);
    assert!(options.transparent);
    assert!(!options.resizable);
    assert!(!options.visible);
    assert!(options.always_on_top);
    assert_eq!(options.min_width, Some(5.0));
    assert_eq!(options.min_height, Some(6.0));
    assert_eq!(options.max_width, Some(50.0));
    assert_eq!(options.max_height, Some(60.0));
}

#[test]
fn host_window_position_records_origin() {
    let options = NativeWindowOptions::new("Panel", 10.0, 20.0).with_host_window_position(3.0, 4.0);
    assert_eq!(options.x, Some(3.0));
    assert_eq!(options.y, Some(4.0));
    assert_eq!(
        options.position_origin,
        NativeWindowPositionOrigin::HostWindow
    );
}

#[test]
fn events_builder_registers_move_callback() {
    let events = NativeWindowEvents::new().with_on_moved(|_, _| {});
    assert!(events.on_moved.is_some());
}

#[test]
fn window_state_accessors_update_position_and_size() {
    let owned = test_window_state(100.0, 50.0);
    let state = owned.state;

    assert_eq!(state.position_non_reactive(), None);
    assert_eq!(state.size_non_reactive(), Size::new(100.0, 50.0));

    state.set_position(Some(Point::new(4.0, 8.0)));
    assert_eq!(state.position_non_reactive(), Some(Point::new(4.0, 8.0)));

    state.translate(3.0, -2.0);
    assert_eq!(state.position_non_reactive(), Some(Point::new(7.0, 6.0)));

    state.set_size(Size::new(120.0, 64.0));
    assert_eq!(state.size_non_reactive(), Size::new(120.0, 64.0));
}

#[test]
fn a_window_state_the_composition_dropped_takes_the_last_present_quietly() {
    let OwnedWindowState {
        _runtime,
        _presented,
        state,
        ..
    } = test_window_state(100.0, 50.0);
    drop(_presented);
    state.set_presented(false);
    assert!(!state.presented.is_alive());
}

#[test]
fn window_state_reports_a_frame_on_the_screen_only_after_a_present() {
    let owned = test_window_state(100.0, 50.0);
    let state = owned.state;

    assert!(
        !state.presented_non_reactive(),
        "a new window has no frame up"
    );
    state.set_presented(true);
    assert!(state.presented_non_reactive());
    state.set_presented(false);
    assert!(
        !state.presented_non_reactive(),
        "a hidden window has no frame up"
    );
}

#[test]
fn window_config_collects_window_settings_and_callbacks() {
    assert!(
        NativeWindowOptions::new("Panel", 100.0, 50.0).shadow,
        "a window keeps the desktop's own drop shadow unless it asks otherwise"
    );
    let config = WindowConfig::borderless("Panel", 100.0, 50.0)
        .with_host_window_position(7.0, 9.0)
        .with_transparent(true)
        .with_shadow(false)
        .with_resizable(false)
        .with_visible(false)
        .with_always_on_top(true)
        .with_focus(WindowFocus::Always)
        .with_min_size(20.0, 10.0)
        .with_max_size(400.0, 200.0)
        .on_moved(|_, _| {})
        .on_resized(|_, _| {})
        .on_close_requested(|| {});

    let NativeWindowParts {
        options,
        events: callbacks,
        state,
        ..
    } = config.into_parts();
    assert_eq!(options.title, "Panel");
    assert_eq!(options.width, 100.0);
    assert_eq!(options.height, 50.0);
    assert_eq!(options.x, Some(7.0));
    assert_eq!(options.y, Some(9.0));
    assert_eq!(
        options.position_origin,
        NativeWindowPositionOrigin::HostWindow
    );
    assert!(!options.decorations);
    assert!(options.transparent);
    assert!(
        !options.shadow,
        "a window that fades out draws its own shadow, not the desktop's"
    );
    assert!(!options.resizable);
    assert!(!options.visible);
    assert!(options.always_on_top);
    assert_eq!(options.focus, WindowFocus::Always);
    assert_eq!(options.min_width, Some(20.0));
    assert_eq!(options.min_height, Some(10.0));
    assert_eq!(options.max_width, Some(400.0));
    assert_eq!(options.max_height, Some(200.0));
    assert!(callbacks.on_moved.is_some());
    assert!(callbacks.on_resized.is_some());
    assert!(callbacks.on_close_requested.is_some());
    assert!(state.is_none());
}

#[test]
fn state_window_configs_bind_size_position() {
    let owned = test_window_state(100.0, 50.0);
    let state = owned.state;
    state.set_position(Some(Point::new(7.0, 9.0)));

    let NativeWindowParts {
        options,
        events: callbacks,
        state: bound_state,
        ..
    } = WindowConfig::borderless_for_state("Panel", state).into_parts();
    assert_eq!(options.title, "Panel");
    assert_eq!(options.width, 100.0);
    assert_eq!(options.height, 50.0);
    assert_eq!(options.x, Some(7.0));
    assert_eq!(options.y, Some(9.0));
    assert!(!options.decorations);
    assert!(!options.resizable);
    assert!(bound_state == Some(state));
    assert!(callbacks.on_moved.is_none());
    assert!(callbacks.on_resized.is_none());

    state.set_size(Size::new(320.0, 200.0));

    let NativeWindowParts {
        options: decorated_options,
        state: decorated_state,
        ..
    } = WindowConfig::new_for_state("Decorated", state).into_parts();
    assert_eq!(decorated_options.width, 320.0);
    assert_eq!(decorated_options.height, 200.0);
    assert!(decorated_options.decorations);
    assert!(decorated_options.resizable);
    assert!(decorated_state == Some(state));
}

#[test]
fn drag_request_reports_missing_handler() {
    assert!(!request_native_window_drag());
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn drag_request_uses_handler_result() {
    let resize_handler: NativeWindowResizeHandler = Rc::new(|_| {});

    with_native_window_drag_handler(Rc::new(|| true), Rc::clone(&resize_handler), || {
        assert!(request_native_window_drag());
    });
    with_native_window_drag_handler(Rc::new(|| false), resize_handler, || {
        assert!(!request_native_window_drag());
    });

    assert!(!request_native_window_drag());
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn native_window_drag_handler_restores_outer_scope_after_nested_scope() {
    let outer_resize_calls = Rc::new(Cell::new(0));
    let inner_resize_calls = Rc::new(Cell::new(0));
    let outer_resize_handler: NativeWindowResizeHandler = {
        let outer_resize_calls = Rc::clone(&outer_resize_calls);
        Rc::new(move |_| outer_resize_calls.set(outer_resize_calls.get() + 1))
    };
    let inner_resize_handler: NativeWindowResizeHandler = {
        let inner_resize_calls = Rc::clone(&inner_resize_calls);
        Rc::new(move |_| inner_resize_calls.set(inner_resize_calls.get() + 1))
    };

    with_native_window_drag_handler(Rc::new(|| true), outer_resize_handler, || {
        assert!(request_native_window_drag());
        assert!(request_native_window_resize(
            WindowResizeDirection::SouthEast
        ));

        with_native_window_drag_handler(Rc::new(|| false), inner_resize_handler, || {
            assert!(!request_native_window_drag());
            assert!(request_native_window_resize(
                WindowResizeDirection::NorthWest
            ));
        });

        assert!(request_native_window_drag());
        assert!(request_native_window_resize(
            WindowResizeDirection::SouthEast
        ));
    });

    assert!(!request_native_window_drag());
    assert_eq!(outer_resize_calls.get(), 2);
    assert_eq!(inner_resize_calls.get(), 1);
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn drag_area_callbacks_follow_accepted_native_drag_lifecycle() {
    use std::cell::Cell;

    use cranpose_ui::{PointerEvent, collect_slices_from_modifier};

    let (_app_context, _app_context_scope) = test_app_context_scope();
    let started = Rc::new(Cell::new(0));
    let finished = Rc::new(Cell::new(0));
    let modifier = Modifier::empty().window_drag_area_with_callbacks(
        {
            let started = Rc::clone(&started);
            move || started.set(started.get() + 1)
        },
        {
            let finished = Rc::clone(&finished);
            move || finished.set(finished.get() + 1)
        },
    );
    let slices = collect_slices_from_modifier(&modifier);
    let handler = slices
        .pointer_inputs()
        .first()
        .expect("window drag pointer handler")
        .clone();
    let resize_handler: NativeWindowResizeHandler = Rc::new(|_| {});

    with_native_window_drag_handler(Rc::new(|| true), resize_handler, || {
        let down = PointerEvent::new(
            PointerEventKind::Down,
            Point::new(4.0, 5.0),
            Point::new(4.0, 5.0),
        );
        handler(down.clone());
        assert!(down.is_consumed());

        handler(PointerEvent::new(
            PointerEventKind::Up,
            Point::new(4.0, 5.0),
            Point::new(4.0, 5.0),
        ));
    });

    assert_eq!(started.get(), 1);
    assert_eq!(finished.get(), 1);
}

#[test]
fn resize_request_reports_missing_handler() {
    assert!(!request_native_window_resize(
        WindowResizeDirection::SouthEast
    ));
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn static_keys_are_stable() {
    assert_eq!(
        NativeWindowKey::from_static("stable"),
        NativeWindowKey::from_static("stable")
    );
    assert_ne!(
        NativeWindowKey::from_static("stable"),
        NativeWindowKey::from_static("other")
    );
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn graph_group(policy: WindowAttachPolicy) -> NativeWindowGroupMembership {
    NativeWindowGroupMembership {
        id: WindowGroupId::from_static("test-group"),
        policy,
        leads: false,
    }
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn lead_group(windows: &mut [WindowGraphPeerSnapshot], leader: WindowId) {
    for window in windows.iter_mut().filter(|window| window.node.id == leader) {
        if let Some(group) = &mut window.group {
            group.leads = true;
        }
    }
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn graph_node(
    id: &'static str,
    position: Point,
    size: Size,
    group: &NativeWindowGroupMembership,
) -> WindowGraphPeerSnapshot {
    WindowGraphPeerSnapshot {
        node: WindowGraphNodeSnapshot {
            id: WindowId::from_static(id),
            position,
            size,
        },
        group: Some(group.clone()),
    }
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
const GRAPH_NODE_SIZE: Size = Size::new(100.0, 50.0);

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn graph_windows(
    group: &NativeWindowGroupMembership,
    nodes: &[(&'static str, Point)],
) -> Vec<WindowGraphPeerSnapshot> {
    nodes
        .iter()
        .map(|(id, position)| graph_node(id, *position, GRAPH_NODE_SIZE, group))
        .collect()
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn main_and_playlist_windows(group: &NativeWindowGroupMembership) -> Vec<WindowGraphPeerSnapshot> {
    graph_windows(
        group,
        &[
            ("main", Point::new(100.0, 100.0)),
            ("playlist", Point::new(216.0, 100.0)),
        ],
    )
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn main_and_eq_windows(group: &NativeWindowGroupMembership) -> Vec<WindowGraphPeerSnapshot> {
    graph_windows(
        group,
        &[
            ("main", Point::new(100.0, 100.0)),
            ("eq", Point::new(100.0, 150.0)),
        ],
    )
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn graph_position(moves: &[WindowGraphMove], id: WindowId) -> Option<Point> {
    moves
        .iter()
        .find(|window_move| window_move.id == id)
        .map(|window_move| window_move.position)
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn graph_drag_capture_freezes_attached_component() {
    let main = WindowId::from_static("main");
    let eq = WindowId::from_static("eq");
    let playlist = WindowId::from_static("playlist");
    let group = graph_group(WindowAttachPolicy::default());
    let windows = graph_windows(
        &group,
        &[
            ("main", Point::new(100.0, 100.0)),
            ("eq", Point::new(100.0, 150.0)),
            ("playlist", Point::new(240.0, 150.0)),
        ],
    );

    let mut graph = WindowGraphState::default();
    graph.start_drag(&windows, main);
    let moves = graph.drag_to(main, Point::new(120.0, 100.0));

    assert_eq!(graph_position(&moves, main), Some(Point::new(120.0, 100.0)));
    assert_eq!(graph_position(&moves, eq), Some(Point::new(120.0, 150.0)));
    assert_eq!(graph_position(&moves, playlist), None);
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn graph_does_not_attach_new_window_during_drag() {
    let main = WindowId::from_static("main");
    let playlist = WindowId::from_static("playlist");
    let group = graph_group(WindowAttachPolicy::default());
    let windows = main_and_playlist_windows(&group);

    let mut graph = WindowGraphState::default();
    graph.start_drag(&windows, main);
    let moves = graph.drag_to(main, Point::new(112.0, 100.0));

    assert_eq!(graph_position(&moves, main), Some(Point::new(112.0, 100.0)));
    assert_eq!(graph_position(&moves, playlist), None);
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn graph_does_not_detach_captured_component_during_fast_drag() {
    let main = WindowId::from_static("main");
    let eq = WindowId::from_static("eq");
    let group = graph_group(WindowAttachPolicy::default());
    let windows = main_and_eq_windows(&group);

    let mut graph = WindowGraphState::default();
    graph.start_drag(&windows, main);
    let moves = graph.drag_to(main, Point::new(400.0, 280.0));

    assert_eq!(graph_position(&moves, main), Some(Point::new(400.0, 280.0)));
    assert_eq!(graph_position(&moves, eq), Some(Point::new(400.0, 330.0)));
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn graph_release_recomputes_attachment_once() {
    let main = WindowId::from_static("main");
    let playlist = WindowId::from_static("playlist");
    let group = graph_group(WindowAttachPolicy::default());
    let start = main_and_playlist_windows(&group);
    let finish = graph_windows(
        &group,
        &[
            ("main", Point::new(112.0, 100.0)),
            ("playlist", Point::new(216.0, 100.0)),
        ],
    );

    let mut graph = WindowGraphState::default();
    graph.start_drag(&start, main);
    let release_moves = graph.finish_drag(&finish);
    let second_release_moves = graph.finish_drag(&finish);

    assert_eq!(
        graph_position(&release_moves, main),
        Some(Point::new(116.0, 100.0))
    );
    assert_eq!(
        graph_position(&release_moves, playlist),
        Some(Point::new(216.0, 100.0))
    );
    assert!(second_release_moves.is_empty());
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn graph_release_uses_drag_start_component_for_attached_windows() {
    let main = WindowId::from_static("main");
    let group = graph_group(WindowAttachPolicy::default());
    let start = graph_windows(
        &group,
        &[
            ("main", Point::new(100.0, 100.0)),
            ("equalizer", Point::new(100.0, 150.0)),
            ("playlist", Point::new(200.0, 150.0)),
        ],
    );
    let finish_with_peer_position_lag = graph_windows(
        &group,
        &[
            ("main", Point::new(112.0, 100.0)),
            ("equalizer", Point::new(112.0, 150.0)),
            ("playlist", Point::new(216.0, 150.0)),
        ],
    );

    let mut graph = WindowGraphState::default();
    graph.start_drag(&start, main);

    assert!(
        graph.finish_drag(&finish_with_peer_position_lag).is_empty(),
        "release snapping must not drop a start-captured peer from the moving component because OS move events arrived out of phase"
    );
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn graph_cancel_drag_discards_active_capture_without_release_moves() {
    let main = WindowId::from_static("main");
    let group = graph_group(WindowAttachPolicy::default());
    let windows = main_and_playlist_windows(&group);

    let mut graph = WindowGraphState::default();
    graph.start_drag(&windows, main);
    graph.cancel_drag();

    assert!(graph.finish_drag(&windows).is_empty());
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn graph_drag_leader_only_moves_attached_component() {
    let main = WindowId::from_static("main");
    let eq = WindowId::from_static("eq");
    let group = graph_group(WindowAttachPolicy::new(
        8.0,
        3.0,
        WindowMoveMode::LeadersOnly,
    ));
    let mut windows = main_and_eq_windows(&group);
    lead_group(&mut windows, main);

    let mut graph = WindowGraphState::default();
    graph.start_drag(&windows, eq);
    let eq_moves = graph.drag_to(eq, Point::new(130.0, 170.0));
    graph.start_drag(&windows, main);
    let main_moves = graph.drag_to(main, Point::new(130.0, 110.0));

    assert_eq!(
        graph_position(&eq_moves, eq),
        Some(Point::new(130.0, 170.0))
    );
    assert_eq!(graph_position(&eq_moves, main), None);
    assert_eq!(
        graph_position(&main_moves, main),
        Some(Point::new(130.0, 110.0))
    );
    assert_eq!(
        graph_position(&main_moves, eq),
        Some(Point::new(130.0, 160.0))
    );
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn test_owner() -> NativeWindowOwner {
    Rc::new(())
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn test_root() -> NativeWindowRootHandle {
    Rc::new(NativeWindowRoot::new(Size::new(1.0, 1.0)))
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn register_panel(key: NativeWindowKey, content: NativeWindowRootHandle, owner: NativeWindowOwner) {
    register_native_window(
        key,
        NativeWindowOptions::new("Panel", 100.0, 50.0),
        NativeWindowEvents::new(),
        None,
        None,
        content,
        owner,
    );
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn register_visible_panel(
    key: NativeWindowKey,
    visible: bool,
    content: NativeWindowRootHandle,
    owner: NativeWindowOwner,
) {
    register_native_window(
        key,
        NativeWindowOptions::new("Panel", 100.0, 50.0).with_visible(visible),
        NativeWindowEvents::new(),
        None,
        None,
        content,
        owner,
    );
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
fn latest_revision(registry: &NativeWindowRegistry) -> u64 {
    native_window_requests(registry)
        .into_iter()
        .next()
        .expect("native window request")
        .revision
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn registry_replaces_native_window_declarations_by_key() {
    with_request_test_registry(|registry| {
        clear_native_window_requests(registry);

        let key = NativeWindowKey::from_static("visibility-update");
        let owner = test_owner();
        let content = test_root();

        register_visible_panel(key, true, Rc::clone(&content), Rc::clone(&owner));
        let initial_revision = latest_revision(registry);

        register_visible_panel(key, false, content, owner);

        let requests = native_window_requests(registry);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].key, key);
        assert_ne!(requests[0].revision, initial_revision);
        assert!(!requests[0].options.visible);

        clear_native_window_requests(registry);
        assert!(native_window_requests(registry).is_empty());
    });
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn registry_revisions_change_on_each_declaration() {
    let (first_revision, second_revision) = revisions_of_two_declarations("content-update", false);
    assert_ne!(first_revision, second_revision);
}

fn revisions_of_two_declarations(key: &'static str, same_root: bool) -> (u64, u64) {
    with_request_test_registry(|registry| {
        clear_native_window_requests(registry);
        let key = NativeWindowKey::from_static(key);
        let owner = test_owner();
        let content = test_root();
        register_panel(key, Rc::clone(&content), Rc::clone(&owner));
        let first_revision = latest_revision(registry);
        let second_root = if same_root { content } else { test_root() };
        register_panel(key, second_root, owner);
        let second_revision = latest_revision(registry);
        clear_native_window_requests(registry);
        (first_revision, second_revision)
    })
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn registry_revisions_change_when_same_content_is_updated() {
    let (first_revision, second_revision) =
        revisions_of_two_declarations("same-content-update", true);
    assert_ne!(first_revision, second_revision);
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn unregister_ignores_stale_owner_after_redeclaration() {
    with_request_test_registry(|registry| {
        clear_native_window_requests(registry);

        let key = NativeWindowKey::from_static("reattach-window");
        let stale_owner = test_owner();
        let current_owner = test_owner();
        let second_content = test_root();

        register_panel(key, test_root(), Rc::clone(&stale_owner));
        register_panel(key, Rc::clone(&second_content), Rc::clone(&current_owner));
        unregister_native_window(key, stale_owner);

        let requests = native_window_requests(registry);
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].key, key);
        assert!(Rc::ptr_eq(&requests[0].root, &second_content));

        unregister_native_window(key, current_owner);
        assert!(native_window_requests(registry).is_empty());

        clear_native_window_requests(registry);
    });
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn clear_does_not_reuse_same_content_revision() {
    with_request_test_registry(|registry| {
        clear_native_window_requests(registry);

        let key = NativeWindowKey::from_static("remove-window");
        let owner = test_owner();
        let content = test_root();

        register_panel(key, Rc::clone(&content), Rc::clone(&owner));
        let first_revision = latest_revision(registry);

        clear_native_window_requests(registry);
        assert!(native_window_requests(registry).is_empty());

        register_panel(key, content, owner);
        let second_revision = latest_revision(registry);

        assert_ne!(first_revision, second_revision);

        clear_native_window_requests(registry);
    });
}
#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[composable(no_skip)]
#[allow(non_snake_case)]
fn TornPageRoot(
    torn: cranpose_core::MutableState<bool>,
    title: cranpose_core::MutableState<&'static str>,
    node: Rc<Cell<Option<cranpose_core::NodeId>>>,
) {
    cranpose_ui::Box(
        Modifier::empty(),
        cranpose_ui::BoxSpec::default(),
        move || {
            let modifier = if torn.get() {
                Modifier::empty().window(WindowConfig::new(title.get(), 100.0, 50.0))
            } else {
                Modifier::empty()
            };
            let id = cranpose_ui::Box(modifier, cranpose_ui::BoxSpec::default(), || {});
            node.set(Some(id));
        },
    );
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
struct TornPage {
    test: RequestTestComposition,
    torn: cranpose_core::MutableState<bool>,
    title: cranpose_core::MutableState<&'static str>,
    node: Rc<Cell<Option<cranpose_core::NodeId>>>,
    root_key: cranpose_core::Key,
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
impl TornPage {
    fn composed() -> Self {
        let test = request_test_composition();
        let torn = cranpose_core::MutableState::with_runtime(true, test.runtime.handle());
        let title = cranpose_core::MutableState::with_runtime("Torn page", test.runtime.handle());
        let node = Rc::new(Cell::new(None));
        let root_key = cranpose_core::location_key(file!(), line!(), column!());
        let mut page = Self {
            test,
            torn,
            title,
            node,
            root_key,
        };
        let (torn, title, node) = (page.torn, page.title, Rc::clone(&page.node));
        page.test.with_registry(|composition| {
            composition
                .render_stable(root_key, move || {
                    TornPageRoot(torn, title, Rc::clone(&node))
                })
                .expect("the torn page renders");
        });
        page
    }

    fn recompose(&mut self) {
        let (torn, title, node) = (self.torn, self.title, Rc::clone(&self.node));
        let root_key = self.root_key;
        self.test.with_registry(|composition| {
            composition
                .reconcile(root_key, move || {
                    TornPageRoot(torn, title, Rc::clone(&node))
                })
                .expect("the torn page recomposes");
        });
    }

    fn node(&self) -> cranpose_core::NodeId {
        self.node.get().expect("the page composed")
    }

    fn only_request(&self) -> NativeWindowRequest {
        let requests = native_window_requests(&self.test.registry);
        assert_eq!(requests.len(), 1, "one window request");
        requests.into_iter().next().expect("one window request")
    }
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn the_window_modifier_requests_a_window_for_its_own_node() {
    let page = TornPage::composed();
    let request = page.only_request();
    assert_eq!(
        request.key.raw(),
        page.node() as u64,
        "the request's key is the node, which names the window's surface root"
    );
    assert_eq!(request.options.title, "Torn page");
    let roots = cranpose_ui::window_roots();
    assert_eq!(roots.len(), 1, "the node is a window root");
    assert_eq!(roots[0].node, page.node());
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn dropping_the_window_modifier_keeps_the_node_and_withdraws_the_request() {
    let mut page = TornPage::composed();
    let node = page.node();
    page.torn.set(false);
    page.recompose();
    assert_eq!(page.node(), node, "the subtree keeps its node inline");
    assert_eq!(request_count(&page.test.registry), 0);
    assert!(cranpose_ui::window_roots().is_empty());

    page.torn.set(true);
    page.recompose();
    assert_eq!(page.node(), node, "the subtree keeps its node in a window");
    assert_eq!(page.only_request().key, WindowId::from_node(node));
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn a_changed_config_reaches_the_request_with_a_new_revision() {
    let mut page = TornPage::composed();
    let first = page.only_request();
    page.title.set("Renamed page");
    page.recompose();
    let second = page.only_request();
    assert_eq!(second.options.title, "Renamed page");
    assert_ne!(second.revision, first.revision);
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn group_and_leadership_reach_the_request() {
    let mut test = request_test_composition();
    let root_key = cranpose_core::location_key(file!(), line!(), column!());
    let policy = WindowAttachPolicy::new(8.0, 3.0, WindowMoveMode::LeadersOnly);
    let config = WindowConfig::new("Grouped", 100.0, 50.0)
        .group("tools", policy.clone())
        .leads_group(true);
    test.with_registry(|composition| {
        composition
            .render_stable(root_key, move || {
                WindowBox(config.clone());
            })
            .expect("the grouped window renders");
    });
    let requests = native_window_requests(&test.registry);
    assert_eq!(
        requests[0].group,
        Some(NativeWindowGroupMembership {
            id: WindowGroupId::from_static("tools"),
            policy,
            leads: true,
        })
    );
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[composable(no_skip)]
#[allow(non_snake_case)]
fn LocalWindowStateRoot(
    torn: cranpose_core::MutableState<bool>,
    declared: Rc<Cell<Option<WindowState>>>,
    inside: Rc<Cell<Option<WindowState>>>,
    beside: Rc<Cell<Option<WindowState>>>,
) {
    let state = rememberWindowStateAt(12.0, 34.0, 100.0, 50.0);
    declared.set(Some(state));
    let modifier = if torn.get() {
        Modifier::empty().window(WindowConfig::borderless_for_state("Local window", state))
    } else {
        Modifier::empty()
    };
    cranpose_ui::Box(modifier, cranpose_ui::BoxSpec::default(), move || {
        inside.set(crate::LocalWindowState::current());
    });
    cranpose_ui::Box(
        Modifier::empty(),
        cranpose_ui::BoxSpec::default(),
        move || {
            beside.set(crate::LocalWindowState::current());
        },
    );
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
struct LocalWindowStatePage {
    test: RequestTestComposition,
    torn: cranpose_core::MutableState<bool>,
    declared: Rc<Cell<Option<WindowState>>>,
    inside: Rc<Cell<Option<WindowState>>>,
    beside: Rc<Cell<Option<WindowState>>>,
    root_key: cranpose_core::Key,
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
impl LocalWindowStatePage {
    fn composed(torn_at_first: bool) -> Self {
        let test = request_test_composition();
        let torn = cranpose_core::MutableState::with_runtime(torn_at_first, test.runtime.handle());
        let mut page = Self {
            test,
            torn,
            declared: Rc::new(Cell::new(None)),
            inside: Rc::new(Cell::new(None)),
            beside: Rc::new(Cell::new(None)),
            root_key: cranpose_core::location_key(file!(), line!(), column!()),
        };
        let root_key = page.root_key;
        let content = page.content();
        page.test.with_registry(|composition| {
            composition
                .render_stable(root_key, content)
                .expect("the window content renders");
        });
        page
    }

    fn content(&self) -> impl FnMut() + 'static {
        let torn = self.torn;
        let declared = Rc::clone(&self.declared);
        let inside = Rc::clone(&self.inside);
        let beside = Rc::clone(&self.beside);
        move || {
            LocalWindowStateRoot(
                torn,
                Rc::clone(&declared),
                Rc::clone(&inside),
                Rc::clone(&beside),
            )
        }
    }

    fn recompose(&mut self) {
        let root_key = self.root_key;
        let content = self.content();
        self.test.with_registry(|composition| {
            composition
                .reconcile(root_key, content)
                .expect("the window content recomposes");
        });
    }
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn window_content_reads_its_own_window_from_the_composition_local() {
    let page = LocalWindowStatePage::composed(true);
    assert_eq!(
        page.inside.get(),
        page.declared.get(),
        "content inside the window reads the state the window was declared with"
    );
    assert_eq!(
        page.beside.get(),
        None,
        "a sibling outside the window is in no window of its own"
    );
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn content_put_back_inline_stops_reading_a_window() {
    let mut page = LocalWindowStatePage::composed(true);
    assert!(page.inside.get().is_some(), "the content starts torn out");
    page.torn.set(false);
    page.recompose();
    assert_eq!(
        page.inside.get(),
        None,
        "the same node put back inline has no window to read"
    );
}

#[cfg(all(
    feature = "desktop-shell",
    feature = "renderer-wgpu",
    not(target_arch = "wasm32")
))]
#[test]
fn content_torn_out_starts_reading_the_window_it_moved_into() {
    let mut page = LocalWindowStatePage::composed(false);
    assert_eq!(page.inside.get(), None, "the content starts inline");
    page.torn.set(true);
    page.recompose();
    assert_eq!(
        page.inside.get(),
        page.declared.get(),
        "the node that grew a window reads the window it grew"
    );
}
