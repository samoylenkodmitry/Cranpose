use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use cranpose_core::MutableState;
use cranpose_ui::{Point, Size, composable};
use thiserror::Error;

use crate::scoped_weak_stack::ScopedWeakStack;

const SIZE_EPSILON: f32 = 0.5;

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
    /// An Android host window's request states, for code that remembers the
    /// state itself.
    ///
    /// Call it inside a `remember`; [`rememberAndroidHostWindowState`] is this
    /// plus the slot, and also the registration the host needs.
    pub fn new(width: f32, height: f32) -> Self {
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
        AndroidHostWindowState {
            requested_size: cranpose_core::mutableStateOf(initial_requested),
            requested_position: cranpose_core::mutableStateOf(Point::ZERO),
            actual_size: cranpose_core::mutableStateOf(Size::ZERO),
            request_revision: cranpose_core::mutableStateOf(initial_revision),
            position_revision: cranpose_core::mutableStateOf(0_u64),
            status: cranpose_core::mutableStateOf(initial_status),
        }
    }

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
#[composable]
#[track_caller]
pub fn rememberAndroidHostWindowState(width: f32, height: f32) -> AndroidHostWindowState {
    let caller = cranpose_core::caller_location_key();
    let state = cranpose_core::remember(move || AndroidHostWindowState::new(width, height))
        .with(|state| *state);

    let owner = cranpose_core::remember(|| Rc::new(())).with(Rc::clone);
    {
        let owner = Rc::clone(&owner);
        cranpose_core::SideEffect(move || {
            register_android_host_window_state(state, owner);
        });
    }
    {
        let owner = Rc::clone(&owner);
        cranpose_core::__disposable_effect_impl(
            caller ^ cranpose_core::location_key(file!(), line!(), column!()),
            (),
            move |scope| {
                scope.on_dispose(move || unregister_android_host_window_state(state, owner))
            },
        );
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

#[derive(Default)]
pub(crate) struct AndroidHostWindowRegistry {
    states: RefCell<Vec<AndroidHostWindowStateRegistration>>,
    next_revision: Cell<u64>,
}

impl AndroidHostWindowRegistry {
    fn latest_request(&self) -> Option<AndroidHostWindowRequest> {
        self.states
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
    }

    fn sync_actual_size(&self, actual: Size) {
        for registration in self.states.borrow().iter() {
            registration.state.set_actual_size(actual);
        }
    }

    fn register(&self, state: AndroidHostWindowState, owner: Rc<()>) {
        let revision = self.next_revision();
        let mut states = self.states.borrow_mut();
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
    }

    fn unregister(&self, state: AndroidHostWindowState, owner: Rc<()>) {
        self.states.borrow_mut().retain(|registration| {
            !(registration.state == state && Rc::ptr_eq(&registration.owner, &owner))
        });
    }

    fn next_revision(&self) -> u64 {
        let current = self.next_revision.get().max(1);
        self.next_revision.set(current.wrapping_add(1).max(1));
        current
    }
}

thread_local! {
    static CURRENT_ANDROID_HOST_WINDOW_REGISTRY: ScopedWeakStack<AndroidHostWindowRegistry> =
        const { ScopedWeakStack::new() };
}

pub(crate) fn with_android_host_window_registry<R>(
    registry: &Rc<AndroidHostWindowRegistry>,
    f: impl FnOnce() -> R,
) -> R {
    CURRENT_ANDROID_HOST_WINDOW_REGISTRY.with(|stack| stack.with_scope(registry, f))
}

fn current_android_host_window_registry() -> Option<Rc<AndroidHostWindowRegistry>> {
    CURRENT_ANDROID_HOST_WINDOW_REGISTRY.with(ScopedWeakStack::current)
}

pub(crate) fn latest_android_host_window_request(
    registry: &AndroidHostWindowRegistry,
) -> Option<AndroidHostWindowRequest> {
    registry.latest_request()
}

pub(crate) fn sync_android_host_window_actual_size(
    registry: &AndroidHostWindowRegistry,
    actual: Size,
) {
    registry.sync_actual_size(actual);
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
    let Some(registry) = current_android_host_window_registry() else {
        log::error!("Android host-window declaration ignored because no registry is active");
        return;
    };
    registry.register(state, owner);
}

fn unregister_android_host_window_state(state: AndroidHostWindowState, owner: Rc<()>) {
    let Some(registry) = current_android_host_window_registry() else {
        log::error!(
            "Android host-window declaration could not unregister because no registry is active"
        );
        return;
    };
    registry.unregister(state, owner);
}

fn logical_dimension_to_physical(logical: f32, density: f32) -> i32 {
    let physical = (logical * density).round();
    physical.clamp(1.0, i32::MAX as f32) as i32
}

#[cfg(test)]
#[path = "tests/android_host_window_tests.rs"]
mod tests;
