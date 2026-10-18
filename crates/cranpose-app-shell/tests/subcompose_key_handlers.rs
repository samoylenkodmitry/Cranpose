//! A key handler on a subcompose layout, such as `BoxWithConstraints`, takes
//! the keys of its focused descendants and its own focus, as a handler on a
//! plain layout does.

use std::{cell::RefCell, rc::Rc};

use cranpose_ui::{
    Box, BoxSpec, BoxWithConstraints, FocusRequester, KeyCode, KeyEvent, Modifier, Size,
};

mod support;
use support::checking_shell;

#[test]
fn a_key_handler_on_box_with_constraints_takes_keys_while_it_is_focused() {
    let pressed: Rc<RefCell<Vec<KeyCode>>> = Rc::default();
    let focus = FocusRequester::new();
    let (sink, requester) = (Rc::clone(&pressed), focus.clone());
    let (mut shell, _) = checking_shell(false, move || {
        let sink = Rc::clone(&sink);
        BoxWithConstraints(
            Modifier::empty()
                .fill_max_size()
                .focus_requester(&requester)
                .focus_target()
                .on_key_event(move |event| {
                    sink.borrow_mut().push(event.key_code);
                    true
                }),
            |_| {
                Box(
                    Modifier::empty().size(Size::new(40.0, 40.0)),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    });
    shell.update();
    shell
        .app_context()
        .enter(|| focus.request_focus())
        .expect("the box takes focus");
    shell.update();

    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::W, "w")));
    assert_eq!(*pressed.borrow(), vec![KeyCode::W]);
}
