//! Button widget implementation

#![allow(non_snake_case)]

use super::nodes::ButtonNode;
use crate::composable;
use crate::modifier::Modifier;
use compose_core::NodeId;
use indexmap::IndexSet;
use std::cell::RefCell;
use std::rc::Rc;

#[composable]
pub fn Button<F, G>(modifier: Modifier, on_click: F, content: G) -> NodeId
where
    F: FnMut() + 'static,
    G: FnMut() + 'static,
{
    let on_click_rc: Rc<RefCell<dyn FnMut()>> = Rc::new(RefCell::new(on_click));

    // Add clickable modifier to connect the on_click handler to pointer input system
    let clickable_modifier = {
        let on_click_for_clickable = on_click_rc.clone();
        modifier.then(Modifier::clickable(move |_point| {
            (on_click_for_clickable.borrow_mut())();
        }))
    };

    let id = compose_core::with_current_composer(|composer| {
        composer.emit_node(|| ButtonNode {
            modifier: clickable_modifier.clone(),
            on_click: on_click_rc.clone(),
            children: IndexSet::new(),
        })
    });
    if let Err(err) = compose_core::with_node_mut(id, |node: &mut ButtonNode| {
        node.modifier = clickable_modifier;
        node.on_click = on_click_rc.clone();
    }) {
        debug_assert!(false, "failed to update Button node: {err}");
    }
    compose_core::push_parent(id);
    content();
    compose_core::pop_parent();
    id
}
