use std::sync::Arc;

use super::*;

struct OwnedWindowState {
    _runtime: cranpose_core::Runtime,
    _position: cranpose_core::OwnedMutableState<Option<Point>>,
    _size: cranpose_core::OwnedMutableState<Size>,
    _frame: cranpose_core::OwnedMutableState<Size>,
    _presented: cranpose_core::OwnedMutableState<bool>,
    state: WindowState,
}

fn test_window_state(width: f32, height: f32) -> OwnedWindowState {
    let runtime = cranpose_core::Runtime::new(Arc::new(cranpose_core::DefaultScheduler));
    let handle = runtime.handle();
    let position = cranpose_core::OwnedMutableState::with_runtime(None::<Point>, handle.clone());
    let size =
        cranpose_core::OwnedMutableState::with_runtime(Size::new(width, height), handle.clone());
    let frame =
        cranpose_core::OwnedMutableState::with_runtime(Size::new(width, height), handle.clone());
    let presented = cranpose_core::OwnedMutableState::with_runtime(false, handle);
    let state = WindowState {
        position: position.handle(),
        size: size.handle(),
        frame: frame.handle(),
        presented: presented.handle(),
    };
    OwnedWindowState {
        _runtime: runtime,
        _position: position,
        _size: size,
        _frame: frame,
        _presented: presented,
        state,
    }
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

    let WindowConfig {
        options,
        callbacks,
        state,
    } = config;
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

    let WindowConfig {
        options,
        callbacks,
        state: bound_state,
    } = WindowConfig::borderless_for_state("Panel", state);
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

    let WindowConfig {
        options: decorated_options,
        state: decorated_state,
        ..
    } = WindowConfig::new_for_state("Decorated", state);
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

#[test]
fn resize_request_reports_missing_handler() {
    assert!(!request_native_window_resize(
        WindowResizeDirection::SouthEast
    ));
}
