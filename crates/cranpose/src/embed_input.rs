use cranpose_app_shell::{AppShell, KeyCode, KeyEvent, KeyEventType, PointerSource, WheelScroll};
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::Point;
use web_time::Instant;

use crate::embed_protocol::HostEvent;

const OUTSIDE_SURFACE: (f32, f32) = (-1.0, -1.0);

pub(crate) struct EmbedInput {
    epoch: Instant,
}

impl EmbedInput {
    pub(crate) fn new() -> Self {
        Self {
            epoch: Instant::now(),
        }
    }

    pub(crate) fn dispatch(&self, shell: &mut AppShell<WgpuRenderer>, event: HostEvent) -> bool {
        match event {
            HostEvent::PointerMove { x, y } => {
                shell.set_pointer_source(PointerSource::Mouse);
                shell.set_cursor(x, y)
            }
            HostEvent::PointerDown { x, y } => {
                shell.set_pointer_source(PointerSource::Mouse);
                let moved = shell.set_cursor(x, y);
                shell.pointer_pressed() || moved
            }
            HostEvent::PointerUp { x, y } => {
                shell.set_pointer_source(PointerSource::Mouse);
                shell.pointer_released_at_position(x, y)
            }
            HostEvent::PointerLeave => {
                !shell.has_active_pointer_gesture()
                    && shell.set_cursor(OUTSIDE_SURFACE.0, OUTSIDE_SURFACE.1)
            }
            HostEvent::Scroll {
                x,
                y,
                delta_x,
                delta_y,
                modifiers,
            } => {
                shell.set_modifiers(modifiers);
                let moved = shell.set_cursor(x, y);
                let scroll = WheelScroll::new(
                    Point {
                        x: delta_x,
                        y: delta_y,
                    },
                    self.uptime_millis(),
                )
                .with_modifiers(modifiers);
                shell.wheel_scrolled(scroll) || moved
            }
            HostEvent::Key {
                down,
                modifiers,
                code,
            } => {
                shell.set_modifiers(modifiers);
                let event_type = if down {
                    KeyEventType::KeyDown
                } else {
                    KeyEventType::KeyUp
                };
                shell.on_key_event(&KeyEvent::new(
                    KeyCode::from_dom_code(&code),
                    "",
                    modifiers,
                    event_type,
                ))
            }
            HostEvent::Text(text) => shell.on_paste(&text),
            HostEvent::Resize { .. }
            | HostEvent::Theme { .. }
            | HostEvent::Message { .. }
            | HostEvent::FrameAck(_)
            | HostEvent::Close
            | HostEvent::Visibility(_) => false,
        }
    }

    fn uptime_millis(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }
}

#[cfg(test)]
#[path = "tests/embed_input_tests.rs"]
mod tests;
