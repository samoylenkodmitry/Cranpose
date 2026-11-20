//! What a space gives the surfaces and volumes that take a ray's events: where each sends them, and
//! the delivery of a gesture as Compose's pointer event and as a volume's input event.

use super::Space;
use crate::surface_input::{InputSink, PointerSink, SurfaceGesture, input_event};

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the delivery of a ray's gestures in its own file"
)]
impl Space {
    // Sets where the surface `id` sends the pointer's events, or `None` for a surface that takes
    // none.
    pub(crate) fn set_surface_sink(&self, id: u64, sink: Option<PointerSink>) {
        let mut entries = self.inner.entries.borrow_mut();
        if let Some(entry) = entries.iter_mut().find(|entry| entry.slot.id == id) {
            entry.sink = sink;
        }
    }

    // Sets where the volume `id` sends the input events of a ray, or `None` for a volume that takes
    // none.
    pub(crate) fn set_input_sink(&self, id: u64, sink: Option<InputSink>) {
        let mut entries = self.inner.entries.borrow_mut();
        if let Some(entry) = entries.iter_mut().find(|entry| entry.slot.id == id) {
            entry.input_sink = sink;
        }
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Tells the surface that `gesture` names what the pointer did on it, as Compose's pointer
    // event, and a volume as the input event of a ray. A surface that takes no pointer events sees
    // nothing.
    pub(crate) fn deliver_to_surface(&self, gesture: &SurfaceGesture) {
        let heard = {
            let mut entries = self.inner.entries.borrow_mut();
            entries
                .iter_mut()
                .find(|entry| entry.slot.id == gesture.surface)
                .and_then(|entry| {
                    let (event, action) = entry.input.event(gesture.gesture)?;
                    let input = entry
                        .input_sink
                        .clone()
                        .zip(gesture.ray)
                        .map(|(sink, ray)| (sink, input_event(&ray, action, &entry.slot.pose)));
                    Some((entry.sink.clone(), event, input))
                })
        };
        // The callbacks run with no borrow held, so that they may call the space
        if let Some((sink, event, input)) = heard {
            if let Some(sink) = sink {
                sink(event);
            }
            if let Some((sink, event)) = input {
                sink(event);
            }
        }
    }
}
