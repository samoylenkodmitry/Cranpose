use std::sync::{Arc, mpsc};

use cranpose::{AppLauncher, TouchRouter, embedded_view::EmbeddedView, prelude::PointerSource};

use crate::{
    NativeContent, NativeContext, NativeError, NativeEvent, NativeFrame, NativeSlot, TouchPhase,
    session::{Command, Wake},
};

struct Component {
    view: EmbeddedView,
    context: NativeContext,
    event: Box<dyn FnMut(NativeEvent)>,
    touches: TouchRouter<u64>,
}

impl Component {
    fn new(content: NativeContent, wake: Arc<Wake>) -> Result<Self, NativeError> {
        let context = NativeContext::new();
        let composition_context = context.clone();
        let NativeContent { mut draw, event } = content;
        let mut view = AppLauncher::new()
            .create_embedded_view(1, 1, 1.0, move || {
                composition_context.provide(&mut draw);
            })
            .map_err(runtime_error)?;
        view.shell().set_frame_waker(move || wake.request());
        Ok(Self {
            view,
            context,
            event,
            touches: TouchRouter::default(),
        })
    }

    fn frame(&mut self, width: u32, height: u32, density: f32) -> Result<NativeFrame, NativeError> {
        self.view
            .resize(width, height, density)
            .map_err(runtime_error)?;
        let mut pixels = Vec::new();
        let changed = self.view.draw(&mut pixels).map_err(runtime_error)?;
        let mut slots = Vec::new();
        if changed {
            self.context.views.for_each_layout(
                self.view.shell().layout_tree(),
                |id, kind, value, bounds| {
                    slots.push(NativeSlot {
                        id,
                        kind: kind.to_owned(),
                        value: value.to_owned(),
                        x: bounds.x,
                        y: bounds.y,
                        width: bounds.width,
                        height: bounds.height,
                    });
                },
            );
        }
        let schedule = self.view.shell().frame_schedule();
        let next_frame_ms = if schedule.needs_frame || schedule.needs_update {
            Some(0)
        } else {
            schedule.next_deadline.map(|deadline| {
                deadline
                    .saturating_duration_since(web_time::Instant::now())
                    .as_millis()
                    .min(u128::from(u64::MAX)) as u64
            })
        };
        Ok(NativeFrame {
            width,
            height,
            pixels,
            slots,
            events: self.context.take_events(),
            next_frame_ms,
        })
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Frame(width, height, density, reply) => {
                let _ = reply.send(self.frame(width, height, density));
            }
            Command::Touch(pointer, phase, x, y) => {
                let shell = self.view.shell();
                shell.set_pointer_source(PointerSource::Touch);
                let finger = Some(pointer);
                match phase {
                    TouchPhase::Down => self.touches.press(shell, finger, x, y),
                    TouchPhase::Move => self.touches.moved(shell, finger, x, y),
                    TouchPhase::Up => self.touches.release(shell, finger, x, y),
                    TouchPhase::Cancel => self.touches.cancel(shell, finger, Some((x, y))),
                };
            }
            Command::Event(event) => (self.event)(event),
            Command::NativeEvent(id, event) => {
                self.context.views.dispatch(id, &event);
            }
            Command::Visible(visible) => self.view.set_visible(visible),
            Command::Close => {}
        }
    }
}

fn runtime_error(error: impl std::fmt::Display) -> NativeError {
    NativeError::Runtime {
        detail: error.to_string(),
    }
}

pub(crate) fn run(
    receiver: mpsc::Receiver<Command>,
    factory: impl FnOnce() -> NativeContent,
    wake: Arc<Wake>,
) {
    let mut component = Component::new(factory(), wake);
    while let Ok(command) = receiver.recv() {
        if matches!(command, Command::Close) {
            break;
        }
        match &mut component {
            Ok(component) => component.handle(command),
            Err(error) => {
                if let Command::Frame(_, _, _, reply) = command {
                    let _ = reply.send(Err(runtime_error(error)));
                }
            }
        }
    }
}
