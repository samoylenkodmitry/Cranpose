//! Who hears of a resize.

use std::{fmt, rc::Rc};

use crate::SpatialResizeEvent;

type OnResize = Rc<dyn Fn(SpatialResizeEvent)>;

/// What a resizable composable does when the user resizes it, as Jetpack XR's `ResizePolicy`.
/// `SubspaceModifier::resizable` takes it.
///
/// The app owns the size. The callback runs on the thread of the composition, once for each
/// resize event, in the frame in which the controller moved. It sets the state that feeds the
/// content's [`SubspaceModifier::size`](crate::SubspaceModifier::size), which lays the content
/// out at the new size and moves the handle to its new corner. A size that the app does not
/// apply stays as it was.
///
/// Jetpack XR's `ResizePolicy.system` and `ResizePolicy.Default`, with which the headset applies
/// the size itself, have no counterpart here.
#[derive(Clone)]
pub struct ResizePolicy {
    on_resize: OnResize,
}

impl ResizePolicy {
    /// The app applies the size: `on_resize` receives each step, and a state that it sets feeds
    /// the content's `SubspaceModifier`. Jetpack XR's `ResizePolicy.custom`.
    pub fn custom(on_resize: impl Fn(SpatialResizeEvent) + 'static) -> Self {
        Self {
            on_resize: Rc::new(on_resize),
        }
    }

    // The callback that hears of the steps, held apart from any borrow so that it may call back.
    pub(crate) fn listener(&self) -> OnResize {
        Rc::clone(&self.on_resize)
    }
}

impl PartialEq for ResizePolicy {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.on_resize, &other.on_resize)
    }
}

impl fmt::Debug for ResizePolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResizePolicy").finish_non_exhaustive()
    }
}
