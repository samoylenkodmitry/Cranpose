use cranpose_app_shell::{KeyCode, KeyEvent, KeyEventType, PointerSource, SurfaceMut, WheelScroll};
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::Point;
use web_time::Instant;

use crate::embed_protocol::SurfaceEvent;

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

    pub(crate) fn dispatch(
        &self,
        surface: &mut SurfaceMut<'_, WgpuRenderer>,
        event: &SurfaceEvent,
    ) -> bool {
        match *event {
            SurfaceEvent::PointerMove { x, y } => {
                surface.set_pointer_source(PointerSource::Mouse);
                surface.set_cursor(x, y)
            }
            SurfaceEvent::PointerDown { x, y } => {
                surface.set_pointer_source(PointerSource::Mouse);
                let moved = surface.set_cursor(x, y);
                surface.pointer_pressed() || moved
            }
            SurfaceEvent::PointerUp { x, y } => {
                surface.set_pointer_source(PointerSource::Mouse);
                surface.pointer_released_at_position(x, y)
            }
            SurfaceEvent::PointerLeave => {
                !surface.has_active_pointer_gesture()
                    && surface.set_cursor(OUTSIDE_SURFACE.0, OUTSIDE_SURFACE.1)
            }
            SurfaceEvent::Scroll {
                x,
                y,
                delta_x,
                delta_y,
                modifiers,
            } => {
                surface.shell().set_modifiers(modifiers);
                let moved = surface.set_cursor(x, y);
                let scroll = WheelScroll::new(
                    Point {
                        x: delta_x,
                        y: delta_y,
                    },
                    self.uptime_millis(),
                )
                .with_modifiers(modifiers);
                surface.wheel_scrolled(scroll) || moved
            }
            SurfaceEvent::Key {
                down,
                modifiers,
                ref code,
            } => {
                surface.shell().set_modifiers(modifiers);
                let event_type = if down {
                    KeyEventType::KeyDown
                } else {
                    KeyEventType::KeyUp
                };
                surface.on_key_event(&KeyEvent::new(
                    KeyCode::from_dom_code(code),
                    "",
                    modifiers,
                    event_type,
                ))
            }
            SurfaceEvent::Text(ref text) => surface.on_paste(text),
            SurfaceEvent::Resize { .. }
            | SurfaceEvent::FrameAck(_)
            | SurfaceEvent::Visibility(_)
            | SurfaceEvent::Moved { .. }
            | SurfaceEvent::CloseRequested => false,
        }
    }

    fn uptime_millis(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }
}

#[cfg(test)]
#[path = "tests/embed_input_tests.rs"]
mod tests;
