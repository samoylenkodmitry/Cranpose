use cranpose_services::{LifecycleState, window_lifecycle_state};
use winit::event::WindowEvent;

pub(crate) struct WindowPresence {
    occluded: bool,
    minimized: bool,
    focused: bool,
    published: LifecycleState,
}

impl WindowPresence {
    pub(crate) fn shown() -> Self {
        Self {
            occluded: false,
            minimized: false,
            focused: true,
            published: LifecycleState::Resumed,
        }
    }

    pub(crate) fn observe(&mut self, event: &WindowEvent) -> Option<LifecycleState> {
        match event {
            WindowEvent::Focused(focused) => self.focused = *focused,
            WindowEvent::Occluded(occluded) => self.occluded = *occluded,
            WindowEvent::SurfaceResized(size) => {
                self.minimized = size.width == 0 || size.height == 0
            }
            _ => return None,
        }
        let state = window_lifecycle_state(!self.occluded && !self.minimized, self.focused);
        if state == self.published {
            return None;
        }
        self.published = state;
        Some(state)
    }
}

#[cfg(test)]
#[path = "tests/desktop_lifecycle.rs"]
mod tests;
