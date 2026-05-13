//! Android host-window size request state.

use cranpose_core::MutableState;
use cranpose_ui::{composable, Point, Size};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};
use thiserror::Error;

const SIZE_EPSILON: f32 = 0.5;

/// Duration after which an Android host-window resize request is considered unsupported.
///
/// Android fullscreen and split-screen activities commonly ignore `Window.setLayout`.
/// A delayed confirmation keeps that distinction observable without blocking the
/// rendering loop while the platform has a chance to emit a resize event.
pub(crate) const HOST_WINDOW_CONFIRMATION_TIMEOUT: Duration = Duration::from_millis(500);

/// Validation error for an Android host-window size request.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AndroidHostWindowSizeError {
    /// Width or height was NaN or infinite.
    #[error("Android host-window dimensions must be finite")]
    NonFinite,
    /// Width or height was zero or negative.
    #[error("Android host-window dimensions must be greater than zero")]
    NonPositive,
}

/// Validation error for an Android host-window position request.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum AndroidHostWindowPositionError {
    /// X or Y was NaN or infinite.
    #[error("Android host-window coordinates must be finite")]
    NonFinite,
}

/// Result status for the latest Android host-window size request.
#[derive(Clone, Debug, PartialEq)]
pub enum AndroidHostWindowSizeStatus {
    /// No host-window size request has been issued by this state.
    Idle,
    /// The request was accepted by Cranpose and is waiting for Android to resize the host surface.
    Pending {
        /// Requested host-window size in logical pixels.
        requested: Size,
    },
    /// Android reported a surface size matching the request.
    Applied {
        /// Requested host-window size in logical pixels.
        requested: Size,
        /// Actual host surface size reported by Android in logical pixels.
        actual: Size,
    },
    /// Android did not report a matching host surface size before the confirmation timeout.
    Unsupported {
        /// Requested host-window size in logical pixels.
        requested: Size,
        /// Actual host surface size reported by Android in logical pixels.
        actual: Size,
    },
    /// The requested dimensions were invalid and were not sent to Android.
    Rejected {
        /// Invalid requested host-window size in logical pixels.
        requested: Size,
        /// Validation failure.
        reason: AndroidHostWindowSizeError,
    },
    /// Cranpose could not send the request to Android.
    DispatchFailed {
        /// Requested host-window size in logical pixels.
        requested: Size,
        /// Platform error message.
        message: String,
    },
}

/// Mutable state for the primary Android host window.
///
/// The requested size is app-owned state. The actual size is updated only from
/// Android surface events, so content layout can shrink independently while the
/// app still observes whether the native host window followed.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AndroidHostWindowState {
    requested_size: MutableState<Size>,
    requested_position: MutableState<Point>,
    actual_size: MutableState<Size>,
    request_revision: MutableState<u64>,
    position_revision: MutableState<u64>,
    status: MutableState<AndroidHostWindowSizeStatus>,
}

impl AndroidHostWindowState {
    /// Returns the requested host-window size in logical pixels.
    pub fn requested_size(self) -> Size {
        self.requested_size.get()
    }

    /// Returns the requested host-window size without subscribing to changes.
    pub fn requested_size_non_reactive(self) -> Size {
        self.requested_size.get_non_reactive()
    }

    /// Returns the requested host-window position in logical pixels.
    ///
    /// Position requests are only applied when Android overlay-window mode is active.
    /// Normal Activity windows keep Android-managed positioning and ignore this value.
    pub fn requested_position(self) -> Point {
        self.requested_position.get()
    }

    /// Returns the requested host-window position without subscribing to changes.
    pub fn requested_position_non_reactive(self) -> Point {
        self.requested_position.get_non_reactive()
    }

    /// Returns the actual host surface size last reported by Android in logical pixels.
    pub fn actual_size(self) -> Size {
        self.actual_size.get()
    }

    /// Returns the actual host surface size without subscribing to changes.
    pub fn actual_size_non_reactive(self) -> Size {
        self.actual_size.get_non_reactive()
    }

    /// Returns the latest request status.
    pub fn status(self) -> AndroidHostWindowSizeStatus {
        self.status.get()
    }

    /// Returns the latest request status without subscribing to changes.
    pub fn status_non_reactive(self) -> AndroidHostWindowSizeStatus {
        self.status.get_non_reactive()
    }

    /// Requests a new Android host-window size in logical pixels.
    ///
    /// The request is sent after the next successful composition pass. Android
    /// fullscreen and split-screen modes may ignore the request; observe
    /// [`AndroidHostWindowState::status`] and
    /// [`AndroidHostWindowState::actual_size`] to distinguish accepted,
    /// applied, and unsupported requests.
    pub fn set_size(self, size: Size) -> Result<(), AndroidHostWindowSizeError> {
        let requested = match validate_logical_size(size) {
            Ok(size) => size,
            Err(reason) => {
                self.status.set(AndroidHostWindowSizeStatus::Rejected {
                    requested: size,
                    reason,
                });
                return Err(reason);
            }
        };
        if self.requested_size.get_non_reactive() != requested {
            self.requested_size.set(requested);
        }
        self.request_revision
            .set(self.request_revision.get_non_reactive().wrapping_add(1));
        self.status
            .set(AndroidHostWindowSizeStatus::Pending { requested });
        Ok(())
    }

    /// Requests a new Android overlay-window position in logical pixels.
    ///
    /// This is meaningful only when the app is launched with
    /// [`crate::AppLauncher::with_android_overlay_window`]. In overlay mode the
    /// request is sent after the next successful composition pass and updates the
    /// `WindowManager.LayoutParams` for the active overlay surface. In normal
    /// Activity mode Android owns task/window placement, so position requests are
    /// retained in state but not dispatched to the platform.
    pub fn set_position(self, position: Point) -> Result<(), AndroidHostWindowPositionError> {
        let requested = validate_logical_position(position)?;
        if self.requested_position.get_non_reactive() != requested {
            self.requested_position.set(requested);
        }
        self.position_revision
            .set(self.position_revision.get_non_reactive().wrapping_add(1));
        Ok(())
    }

    pub(crate) fn request_revision_non_reactive(self) -> u64 {
        self.request_revision.get_non_reactive()
    }

    pub(crate) fn position_revision_non_reactive(self) -> u64 {
        self.position_revision.get_non_reactive()
    }

    pub(crate) fn mark_pending(self, requested: Size) {
        if self.status.get_non_reactive() != (AndroidHostWindowSizeStatus::Pending { requested }) {
            self.status
                .set(AndroidHostWindowSizeStatus::Pending { requested });
        }
    }

    pub(crate) fn mark_applied(self, requested: Size, actual: Size) {
        let next = AndroidHostWindowSizeStatus::Applied { requested, actual };
        if self.status.get_non_reactive() != next {
            self.status.set(next);
        }
    }

    pub(crate) fn mark_unsupported(self, requested: Size, actual: Size) {
        let next = AndroidHostWindowSizeStatus::Unsupported { requested, actual };
        if self.status.get_non_reactive() != next {
            self.status.set(next);
        }
    }

    pub(crate) fn mark_dispatch_failed(self, requested: Size, message: impl Into<String>) {
        self.status
            .set(AndroidHostWindowSizeStatus::DispatchFailed {
                requested,
                message: message.into(),
            });
    }

    fn set_actual_size(self, actual: Size) {
        if self.actual_size.get_non_reactive() != actual {
            self.actual_size.set(actual);
        }
    }
}

/// Remembers Android host-window request state across recompositions.
///
/// The initial dimensions are logical pixels. Declaring this state opts the app
/// into best-effort Android host-window sizing: freeform and desktop-windowing
/// activities can honor the request, while fullscreen and split-screen modes
/// usually keep the system-managed bounds and report
/// [`AndroidHostWindowSizeStatus::Unsupported`].
///
/// In normal Android activity mode this state targets the current
/// `NativeActivity` host window. In Android overlay mode it resizes the active
/// overlay surface and can move it through `WindowManager.updateViewLayout`.
#[allow(non_snake_case)]
#[composable]
pub fn rememberAndroidHostWindowState(width: f32, height: f32) -> AndroidHostWindowState {
    let requested = Size::new(width, height);
    let (initial_requested, initial_status, initial_revision) =
        match validate_logical_size(requested) {
            Ok(size) => (
                size,
                AndroidHostWindowSizeStatus::Pending { requested: size },
                1,
            ),
            Err(reason) => (
                Size::ZERO,
                AndroidHostWindowSizeStatus::Rejected { requested, reason },
                0,
            ),
        };

    let state = AndroidHostWindowState {
        requested_size: cranpose_core::useState(move || initial_requested),
        requested_position: cranpose_core::useState(|| Point::ZERO),
        actual_size: cranpose_core::useState(|| Size::ZERO),
        request_revision: cranpose_core::useState(move || initial_revision),
        position_revision: cranpose_core::useState(|| 0_u64),
        status: cranpose_core::useState(move || initial_status),
    };

    let owner = cranpose_core::remember(|| Rc::new(())).with(Rc::clone);
    {
        let owner = Rc::clone(&owner);
        cranpose_core::SideEffect(move || {
            register_android_host_window_state(state, owner);
        });
    }
    {
        let owner = Rc::clone(&owner);
        cranpose_core::DisposableEffect!((), move |scope| {
            scope.on_dispose(move || unregister_android_host_window_state(state, owner))
        });
    }

    state
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct AndroidHostWindowRequest {
    pub(crate) state: AndroidHostWindowState,
    pub(crate) size: Size,
    pub(crate) position: Point,
    pub(crate) size_revision: u64,
    pub(crate) position_revision: u64,
}

#[derive(Clone)]
struct AndroidHostWindowStateRegistration {
    state: AndroidHostWindowState,
    owner: Rc<()>,
    revision: u64,
}

thread_local! {
    static ANDROID_HOST_WINDOW_STATES: RefCell<Vec<AndroidHostWindowStateRegistration>> =
        const { RefCell::new(Vec::new()) };
    static NEXT_ANDROID_HOST_WINDOW_REVISION: Cell<u64> = const { Cell::new(1) };
}

pub(crate) fn latest_android_host_window_request() -> Option<AndroidHostWindowRequest> {
    ANDROID_HOST_WINDOW_STATES.with(|states| {
        states
            .borrow()
            .iter()
            .filter(|registration| registration.state.request_revision_non_reactive() != 0)
            .max_by_key(|registration| registration.revision)
            .map(|registration| AndroidHostWindowRequest {
                state: registration.state,
                size: registration.state.requested_size_non_reactive(),
                position: registration.state.requested_position_non_reactive(),
                size_revision: registration.state.request_revision_non_reactive(),
                position_revision: registration.state.position_revision_non_reactive(),
            })
    })
}

pub(crate) fn sync_android_host_window_actual_size(actual: Size) {
    ANDROID_HOST_WINDOW_STATES.with(|states| {
        for registration in states.borrow().iter() {
            registration.state.set_actual_size(actual);
        }
    });
}

pub(crate) fn validate_logical_size(size: Size) -> Result<Size, AndroidHostWindowSizeError> {
    if !size.width.is_finite() || !size.height.is_finite() {
        return Err(AndroidHostWindowSizeError::NonFinite);
    }
    if size.width <= 0.0 || size.height <= 0.0 {
        return Err(AndroidHostWindowSizeError::NonPositive);
    }
    Ok(size)
}

pub(crate) fn validate_logical_position(
    position: Point,
) -> Result<Point, AndroidHostWindowPositionError> {
    if !position.x.is_finite() || !position.y.is_finite() {
        return Err(AndroidHostWindowPositionError::NonFinite);
    }
    Ok(position)
}

pub(crate) fn logical_to_physical_window_size(size: Size, density: f32) -> (i32, i32) {
    let scale = if density.is_finite() && density > 0.0 {
        density
    } else {
        1.0
    };
    (
        logical_dimension_to_physical(size.width, scale),
        logical_dimension_to_physical(size.height, scale),
    )
}

pub(crate) fn sizes_match(requested: Size, actual: Size) -> bool {
    (requested.width - actual.width).abs() <= SIZE_EPSILON
        && (requested.height - actual.height).abs() <= SIZE_EPSILON
}

fn register_android_host_window_state(state: AndroidHostWindowState, owner: Rc<()>) {
    let revision = next_android_host_window_revision();
    ANDROID_HOST_WINDOW_STATES.with(|states| {
        let mut states = states.borrow_mut();
        if let Some(existing) = states
            .iter_mut()
            .find(|registration| Rc::ptr_eq(&registration.owner, &owner))
        {
            existing.state = state;
            existing.revision = revision;
        } else {
            states.push(AndroidHostWindowStateRegistration {
                state,
                owner,
                revision,
            });
        }
    });
}

fn unregister_android_host_window_state(state: AndroidHostWindowState, owner: Rc<()>) {
    ANDROID_HOST_WINDOW_STATES.with(|states| {
        states.borrow_mut().retain(|registration| {
            !(registration.state == state && Rc::ptr_eq(&registration.owner, &owner))
        });
    });
}

fn next_android_host_window_revision() -> u64 {
    NEXT_ANDROID_HOST_WINDOW_REVISION.with(|revision| {
        let next = revision.get();
        revision.set(next.wrapping_add(1));
        next
    })
}

fn logical_dimension_to_physical(logical: f32, density: f32) -> i32 {
    let physical = (logical * density).round();
    physical.clamp(1.0, i32::MAX as f32) as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

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
        let requested = cranpose_core::OwnedMutableState::with_runtime(
            Size::new(width, height),
            handle.clone(),
        );
        let position = cranpose_core::OwnedMutableState::with_runtime(Point::ZERO, handle.clone());
        let actual = cranpose_core::OwnedMutableState::with_runtime(Size::ZERO, handle.clone());
        let revision = cranpose_core::OwnedMutableState::with_runtime(1_u64, handle.clone());
        let position_revision =
            cranpose_core::OwnedMutableState::with_runtime(0_u64, handle.clone());
        let status = cranpose_core::OwnedMutableState::with_runtime(
            AndroidHostWindowSizeStatus::Idle,
            handle,
        );
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
        let harness = test_state(100.0, 50.0);
        let state = harness.state;
        state.set_position(Point::new(24.0, 36.0)).unwrap();
        let owner = Rc::new(());
        register_android_host_window_state(state, Rc::clone(&owner));

        let request = latest_android_host_window_request().expect("registered request");

        assert_eq!(request.size, Size::new(100.0, 50.0));
        assert_eq!(request.position, Point::new(24.0, 36.0));
        assert_eq!(request.size_revision, 1);
        assert_eq!(request.position_revision, 1);
        unregister_android_host_window_state(state, owner);
    }
}
