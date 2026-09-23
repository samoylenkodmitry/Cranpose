use cranpose_services::{LifecycleState, window_lifecycle_state};
use winit::{event::WindowEvent, window::WindowId};

pub(crate) struct WindowPresence {
    primary: WindowId,
    focused: Vec<WindowId>,
    occluded: bool,
    minimized: bool,
    published: LifecycleState,
}

impl WindowPresence {
    pub(crate) fn shown(primary: WindowId) -> Self {
        Self {
            primary,
            focused: vec![primary],
            occluded: false,
            minimized: false,
            published: LifecycleState::Resumed,
        }
    }

    pub(crate) fn observe(&mut self, window: WindowId, event: &WindowEvent) {
        let primary = window == self.primary;
        match event {
            WindowEvent::Focused(true) if !self.focused.contains(&window) => {
                self.focused.push(window);
            }
            WindowEvent::Focused(false) | WindowEvent::Destroyed => {
                self.focused.retain(|focused| *focused != window);
            }
            WindowEvent::Occluded(occluded) if primary => self.occluded = *occluded,
            WindowEvent::SurfaceResized(size) if primary => {
                self.minimized = size.width == 0 || size.height == 0;
            }
            _ => {}
        }
    }

    pub(crate) fn settled(&mut self) -> Option<LifecycleState> {
        let visible = !self.occluded && !self.minimized;
        let state = window_lifecycle_state(visible, !self.focused.is_empty());
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
