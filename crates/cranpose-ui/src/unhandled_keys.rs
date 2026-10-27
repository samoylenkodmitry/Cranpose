use std::{cell::RefCell, rc::Rc};

use cranpose_core::{DisposableEffect, remember};
use cranpose_macros::composable;

use crate::KeyEvent;

pub(crate) type KeyHandler = Rc<RefCell<Rc<dyn Fn(&KeyEvent) -> bool>>>;

/// Hands `handler` every key event no control took: none was focused, a
/// focused control let the key pass, and no text field is taking typing.
///
/// This is where an application puts the shortcuts that work anywhere in it,
/// a media player's play and stop keys, for instance. The handler returns
/// `true` for a key it used. While several are composed, the one composed
/// last is asked first, and a key goes no further once one takes it.
///
/// ```ignore
/// UnhandledKeyEvents(move |event| {
///     if event.type_() != KeyEventType::KeyDown {
///         return false;
///     }
///     match event.key() {
///         Key::X => { play(); true }
///         Key::V => { stop(); true }
///         _ => false,
///     }
/// });
/// ```
#[composable]
pub fn UnhandledKeyEvents(handler: impl Fn(&KeyEvent) -> bool + 'static) {
    let latest: KeyHandler = remember(|| {
        let placeholder: Rc<dyn Fn(&KeyEvent) -> bool> = Rc::new(|_: &KeyEvent| false);
        Rc::new(RefCell::new(placeholder))
    })
    .with(Rc::clone);
    *latest.borrow_mut() = Rc::new(handler);
    DisposableEffect((), move |scope| {
        let id = crate::thread_state::cells(|cells| {
            let id = cells.last_unhandled_key_handler_id.get() + 1;
            cells.last_unhandled_key_handler_id.set(id);
            id
        });
        crate::thread_state::with(|state| {
            state.unhandled_key_handlers.borrow_mut().push((id, latest));
        });
        scope.on_dispose(move || {
            crate::thread_state::try_with(|state| {
                state
                    .unhandled_key_handlers
                    .borrow_mut()
                    .retain(|(held, _)| *held != id);
            });
        })
    });
}

/// Offers `event` to the [`UnhandledKeyEvents`] handlers, the one composed
/// last first, and says whether one of them took it. The platform shells
/// call this with each key the focused controls and text fields let pass.
pub fn dispatch_unhandled_key_event(event: &KeyEvent) -> bool {
    let handlers: Vec<KeyHandler> = crate::thread_state::with(|state| {
        state
            .unhandled_key_handlers
            .borrow()
            .iter()
            .rev()
            .map(|(_, handler)| Rc::clone(handler))
            .collect()
    });
    handlers.into_iter().any(|handler| {
        let current = Rc::clone(&handler.borrow());
        current(event)
    })
}
