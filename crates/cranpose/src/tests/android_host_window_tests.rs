use std::sync::Arc;

use super::*;

struct TestState {
    _runtime: cranpose_core::Runtime,
    _requested: cranpose_core::OwnedMutableState<Size>,
    _position: cranpose_core::OwnedMutableState<Point>,
    _actual: cranpose_core::OwnedMutableState<Size>,
    _revision: cranpose_core::OwnedMutableState<u64>,
    _position_revision: cranpose_core::OwnedMutableState<u64>,
    _status: cranpose_core::OwnedMutableState<AndroidHostWindowSizeStatus>,
    state: AndroidHostWindowState,
}

fn test_state(width: f32, height: f32) -> TestState {
    let runtime = cranpose_core::Runtime::new(Arc::new(cranpose_core::DefaultScheduler));
    let handle = runtime.handle();
    let requested =
        cranpose_core::OwnedMutableState::with_runtime(Size::new(width, height), handle.clone());
    let position = cranpose_core::OwnedMutableState::with_runtime(Point::ZERO, handle.clone());
    let actual = cranpose_core::OwnedMutableState::with_runtime(Size::ZERO, handle.clone());
    let revision = cranpose_core::OwnedMutableState::with_runtime(1_u64, handle.clone());
    let position_revision = cranpose_core::OwnedMutableState::with_runtime(0_u64, handle.clone());
    let status =
        cranpose_core::OwnedMutableState::with_runtime(AndroidHostWindowSizeStatus::Idle, handle);
    let state = AndroidHostWindowState {
        requested_size: requested.handle(),
        requested_position: position.handle(),
        actual_size: actual.handle(),
        request_revision: revision.handle(),
        position_revision: position_revision.handle(),
        status: status.handle(),
    };
    TestState {
        _runtime: runtime,
        _requested: requested,
        _position: position,
        _actual: actual,
        _revision: revision,
        _position_revision: position_revision,
        _status: status,
        state,
    }
}

#[test]
fn validate_logical_size_accepts_positive_finite_dimensions() {
    let size = Size::new(275.0, 348.0);

    assert_eq!(validate_logical_size(size), Ok(size));
}

#[test]
fn validate_logical_size_rejects_non_finite_dimensions() {
    assert_eq!(
        validate_logical_size(Size::new(f32::NAN, 100.0)),
        Err(AndroidHostWindowSizeError::NonFinite)
    );
    assert_eq!(
        validate_logical_size(Size::new(100.0, f32::INFINITY)),
        Err(AndroidHostWindowSizeError::NonFinite)
    );
}

#[test]
fn validate_logical_size_rejects_non_positive_dimensions() {
    assert_eq!(
        validate_logical_size(Size::new(0.0, 100.0)),
        Err(AndroidHostWindowSizeError::NonPositive)
    );
    assert_eq!(
        validate_logical_size(Size::new(100.0, -1.0)),
        Err(AndroidHostWindowSizeError::NonPositive)
    );
}

#[test]
fn validate_logical_position_accepts_finite_coordinates() {
    let position = Point::new(-20.0, 48.5);

    assert_eq!(validate_logical_position(position), Ok(position));
}

#[test]
fn validate_logical_position_rejects_non_finite_coordinates() {
    assert_eq!(
        validate_logical_position(Point::new(f32::NAN, 10.0)),
        Err(AndroidHostWindowPositionError::NonFinite)
    );
    assert_eq!(
        validate_logical_position(Point::new(10.0, f32::INFINITY)),
        Err(AndroidHostWindowPositionError::NonFinite)
    );
}

#[test]
fn logical_to_physical_window_size_rounds_and_clamps() {
    assert_eq!(
        logical_to_physical_window_size(Size::new(10.4, 12.6), 2.0),
        (21, 25)
    );
    assert_eq!(
        logical_to_physical_window_size(Size::new(0.1, 0.1), 0.0),
        (1, 1)
    );
}

#[test]
fn state_set_size_updates_requested_size_and_revision() {
    let harness = test_state(100.0, 50.0);
    let state = harness.state;

    state.set_size(Size::new(200.0, 75.0)).unwrap();

    assert_eq!(state.requested_size_non_reactive(), Size::new(200.0, 75.0));
    assert_eq!(state.request_revision_non_reactive(), 2);
    assert_eq!(
        state.status_non_reactive(),
        AndroidHostWindowSizeStatus::Pending {
            requested: Size::new(200.0, 75.0)
        }
    );
}

#[test]
fn state_set_size_rejects_invalid_size_without_changing_request() {
    let harness = test_state(100.0, 50.0);
    let state = harness.state;

    let result = state.set_size(Size::new(f32::NAN, 75.0));

    assert_eq!(result, Err(AndroidHostWindowSizeError::NonFinite));
    assert_eq!(state.requested_size_non_reactive(), Size::new(100.0, 50.0));
    assert_eq!(state.request_revision_non_reactive(), 1);
    match state.status_non_reactive() {
        AndroidHostWindowSizeStatus::Rejected { requested, reason } => {
            assert!(requested.width.is_nan());
            assert_eq!(requested.height, 75.0);
            assert_eq!(reason, AndroidHostWindowSizeError::NonFinite);
        }
        status => panic!("expected rejected status, got {status:?}"),
    }
}

#[test]
fn state_set_position_updates_requested_position_and_revision() {
    let harness = test_state(100.0, 50.0);
    let state = harness.state;

    state.set_position(Point::new(12.0, -4.0)).unwrap();

    assert_eq!(
        state.requested_position_non_reactive(),
        Point::new(12.0, -4.0)
    );
    assert_eq!(state.request_revision_non_reactive(), 1);
    assert_eq!(state.position_revision_non_reactive(), 1);
}

#[test]
fn state_set_position_rejects_invalid_position_without_changing_request() {
    let harness = test_state(100.0, 50.0);
    let state = harness.state;

    let result = state.set_position(Point::new(f32::NAN, 4.0));

    assert_eq!(result, Err(AndroidHostWindowPositionError::NonFinite));
    assert_eq!(state.requested_position_non_reactive(), Point::ZERO);
    assert_eq!(state.request_revision_non_reactive(), 1);
    assert_eq!(state.position_revision_non_reactive(), 0);
}

#[test]
fn state_tracks_actual_size_separately_from_requested_size() {
    let harness = test_state(100.0, 50.0);
    let state = harness.state;

    state.set_size(Size::new(200.0, 75.0)).unwrap();
    state.set_actual_size(Size::new(120.0, 60.0));

    assert_eq!(state.requested_size_non_reactive(), Size::new(200.0, 75.0));
    assert_eq!(state.actual_size_non_reactive(), Size::new(120.0, 60.0));
}

#[test]
fn sizes_match_allows_half_logical_pixel_rounding_error() {
    assert!(sizes_match(Size::new(100.0, 50.0), Size::new(100.5, 49.5)));
    assert!(!sizes_match(Size::new(100.0, 50.0), Size::new(100.6, 50.0)));
}

#[test]
fn latest_request_includes_requested_position() {
    let registry = Rc::new(AndroidHostWindowRegistry::default());
    let harness = test_state(100.0, 50.0);
    let state = harness.state;
    state.set_position(Point::new(24.0, 36.0)).unwrap();
    let owner = Rc::new(());
    with_android_host_window_registry(&registry, || {
        register_android_host_window_state(state, Rc::clone(&owner));
    });

    let request = latest_android_host_window_request(&registry).expect("registered request");

    assert_eq!(request.size, Size::new(100.0, 50.0));
    assert_eq!(request.position, Point::new(24.0, 36.0));
    assert_eq!(request.size_revision, 1);
    assert_eq!(request.position_revision, 1);
    with_android_host_window_registry(&registry, || {
        unregister_android_host_window_state(state, owner);
    });
}

#[test]
fn host_window_requests_are_isolated_by_registry() {
    let first_registry = Rc::new(AndroidHostWindowRegistry::default());
    let second_registry = Rc::new(AndroidHostWindowRegistry::default());
    let first_harness = test_state(100.0, 50.0);
    let second_harness = test_state(160.0, 80.0);
    let first = first_harness.state;
    let second = second_harness.state;
    let first_owner = Rc::new(());
    let second_owner = Rc::new(());

    with_android_host_window_registry(&first_registry, || {
        register_android_host_window_state(first, first_owner);
    });
    with_android_host_window_registry(&second_registry, || {
        register_android_host_window_state(second, second_owner);
    });

    let first_request = latest_android_host_window_request(&first_registry).expect("first request");
    let second_request =
        latest_android_host_window_request(&second_registry).expect("second request");

    assert!(first_request.state == first);
    assert_eq!(first_request.size, Size::new(100.0, 50.0));
    assert!(second_request.state == second);
    assert_eq!(second_request.size, Size::new(160.0, 80.0));
}
