//! A record of the pointer events that reach a node, for a test that asks which pointer made what.

use std::{cell::RefCell, rc::Rc};

use cranpose_ui::{
    Modifier, Point, PointerButtons, PointerEvent, PointerEventKind, PointerId, PointerInputScope,
};

/// What a node saw of one pointer event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeenPointerEvent {
    /// What the pointer did.
    pub kind: PointerEventKind,
    /// The pointer that did it.
    pub id: PointerId,
    /// The buttons that the pointer held after the event.
    pub buttons: PointerButtons,
    /// Where the pointer was, in the node's own pixels.
    pub position: Point,
    /// How far a scroll went, or zero for any other event.
    pub scroll_delta: Point,
}

/// The pointer events that reached the nodes which carry its [`recorder`](Self::recorder), in
/// the order that they came. A clone shares the record.
#[derive(Clone, Debug, Default)]
pub struct PointerLog {
    seen: Rc<RefCell<Vec<SeenPointerEvent>>>,
}

impl PointerLog {
    /// A modifier that records every pointer event that reaches its node, and consumes none.
    pub fn recorder(&self) -> Modifier {
        let seen = Rc::clone(&self.seen);
        Modifier::empty().pointer_input((), move |scope: PointerInputScope| {
            let seen = Rc::clone(&seen);
            async move {
                scope
                    .await_pointer_event_scope(|await_scope| async move {
                        loop {
                            let event: PointerEvent = await_scope.await_pointer_event().await;
                            seen.borrow_mut().push(SeenPointerEvent {
                                kind: event.kind,
                                id: event.id,
                                buttons: event.buttons,
                                position: event.position,
                                scroll_delta: event.scroll_delta,
                            });
                        }
                    })
                    .await;
            }
        })
    }

    /// The events so far.
    pub fn events(&self) -> Vec<SeenPointerEvent> {
        self.seen.borrow().clone()
    }

    /// The pointers that made an event of `kind`, in order.
    pub fn pointers_of(&self, kind: PointerEventKind) -> Vec<PointerId> {
        self.seen
            .borrow()
            .iter()
            .filter(|seen| seen.kind == kind)
            .map(|seen| seen.id)
            .collect()
    }
}
