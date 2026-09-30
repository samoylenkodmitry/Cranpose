//! App-scoped mouse movement through the host's normal input dispatch.

use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use cranpose_core::{RuntimeHandle, current_runtime_handle};
use cranpose_ui_graphics::Point;
use smallvec::SmallVec;

use crate::render_state::{AppContext, require_current_app_context};

/// The surface that receives an injected mouse sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseInputTarget {
    /// The app's primary window.
    Primary,
    /// The surface identified by `Modifier::window_root`.
    Window(u64),
}

/// A mouse movement waiting for the host to dispatch it.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct MouseMoveRequest {
    pub target: MouseInputTarget,
    pub position: Point,
}

#[derive(Default)]
pub(crate) struct MouseInputQueue(RefCell<SmallVec<[MouseMoveRequest; 2]>>);

impl MouseInputQueue {
    fn push(&self, request: MouseMoveRequest) {
        let mut pending = self.0.borrow_mut();
        if let Some(previous) = pending
            .iter_mut()
            .find(|item| item.target == request.target)
        {
            *previous = request;
        } else {
            pending.push(request);
        }
    }

    pub(crate) fn take(&self) -> Option<MouseMoveRequest> {
        let mut pending = self.0.borrow_mut();
        if pending.is_empty() {
            None
        } else {
            Some(pending.remove(0))
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.0.borrow().len()
    }
}

/// Sends mouse movements to the app that created this handle.
///
/// Requests made before a frame are coalesced to the last position per surface.
/// The host dispatches them through ordinary hit testing, enter/exit handling,
/// and active pointer capture. This does not move the operating system cursor.
#[derive(Clone)]
pub struct MouseInput {
    context: Weak<AppContext>,
    runtime: Option<RuntimeHandle>,
}

impl MouseInput {
    /// Requests a mouse movement in the target surface's logical coordinates.
    /// Graphics-layer transforms must already have been applied to `position`.
    /// Non-finite positions and requests after the app closes are ignored.
    pub fn move_to(&self, target: MouseInputTarget, position: Point) {
        if !position.x.is_finite() || !position.y.is_finite() {
            return;
        }
        if let Some(context) = self.context.upgrade() {
            context
                .mouse_input()
                .push(MouseMoveRequest { target, position });
            if let Some(runtime) = &self.runtime {
                runtime.schedule();
            }
        }
    }
}

/// Returns a mouse-input handle for the current app context.
/// Obtain it during composition so requests also wake that app's runtime.
pub fn local_mouse_input() -> MouseInput {
    MouseInput {
        context: Rc::downgrade(&require_current_app_context("mouse input")),
        runtime: current_runtime_handle(),
    }
}
