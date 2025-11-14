//! Button widget implementation

#![allow(non_snake_case)]

use super::nodes::ButtonNode;
use crate::composable;
use crate::modifier::Modifier;
use compose_core::NodeId;
use std::cell::RefCell;
use std::rc::Rc;

#[composable]
pub fn Button<F, G>(modifier: Modifier, on_click: F, content: G) -> NodeId
where
    F: FnMut() + 'static,
    G: FnMut() + 'static,
{
    let on_click_rc: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(on_click));
    let on_click_for_modifier = on_click_rc.clone();

    let clickable_modifier = modifier.then(Modifier::clickable(move |_point| {
        (on_click_for_modifier.borrow_mut())();
    }));

    let id = compose_core::with_current_composer(|composer| {
        composer.emit_node(|| {
            let mut node = ButtonNode::default();
            node.modifier = clickable_modifier.clone();
            node.on_click = on_click_rc.clone();
            node
        })
    });
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut ButtonNode| {
        node.modifier = clickable_modifier;
        node.on_click = on_click_rc;
    }) {
        debug_assert!(false, "failed to update Button node: {err}");
    }
    compose_core::push_parent(id);
    content();
    compose_core::pop_parent();
    id
}
