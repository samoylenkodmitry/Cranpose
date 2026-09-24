use std::rc::Rc;

use super::*;
use crate::{
    LayoutBox, Modifier, Point, Rect, Size,
    layout::{LayoutNodeData, LayoutNodeKind},
};

struct Handle;

impl crate::focus_dispatch::FocusTargetHandle for Handle {
    fn set_focus_state(&self, _state: cranpose_foundation::FocusState) {}
}

fn node(id: NodeId, modifier: Modifier, children: Vec<LayoutBox>) -> LayoutBox {
    if children.is_empty() {
        focus_dispatch::register_focus_target(id, Rc::new(Handle));
    }
    let semantics = crate::modifier::collect_semantics_from_modifier(&modifier).map(Rc::new);
    LayoutBox::new(
        id,
        Rect {
            x: id as f32 * 50.0,
            y: 0.0,
            width: 48.0,
            height: 48.0,
        },
        Point::default(),
        LayoutNodeData::new(
            modifier,
            Default::default(),
            Rc::default(),
            semantics,
            LayoutNodeKind::Layout,
        ),
        children,
    )
}

fn tab(id: NodeId, selected: bool, enabled: bool) -> LayoutBox {
    node(
        id,
        Modifier::empty().semantics(move |config| {
            config.role = Some(SemanticsWidgetRole::Tab);
            config.selected = Some(selected);
            config.enabled = enabled;
        }),
        Vec::new(),
    )
}

#[test]
fn tabs_keep_selection_when_arrows_wrap_and_leave_in_one_step() {
    let _scope = crate::render_state::app_context_test_scope();
    let tree = LayoutTree::new(node(
        1,
        Modifier::empty(),
        vec![
            node(
                2,
                Modifier::empty().selectable_group(),
                vec![
                    tab(3, false, true),
                    tab(4, true, true),
                    tab(5, false, false),
                ],
            ),
            node(6, Modifier::empty(), Vec::new()),
        ],
    ));
    for (current, key, shift, expected) in [
        (None, KeyCode::Tab, false, 4),
        (Some(4), KeyCode::ArrowRight, false, 3),
        (Some(3), KeyCode::ArrowLeft, false, 4),
        (Some(3), KeyCode::End, false, 4),
        (Some(4), KeyCode::Home, false, 3),
        (Some(3), KeyCode::Tab, false, 6),
        (Some(6), KeyCode::Tab, true, 4),
    ] {
        assert_eq!(
            keyboard_focus_target(&tree, current, key, shift),
            Some(KeyboardFocusTarget {
                node_id: expected,
                activate: false
            })
        );
    }
    assert_eq!(
        keyboard_focus_target(&tree, Some(4), KeyCode::ArrowDown, false),
        None
    );
    assert_eq!(
        keyboard_focus_target(&tree, Some(6), KeyCode::Home, false),
        None
    );
}

#[test]
fn nested_groups_hidden_content_and_modals_have_separate_focus_scopes() {
    let _scope = crate::render_state::app_context_test_scope();
    let tree = LayoutTree::new(node(
        1,
        Modifier::empty(),
        vec![
            node(2, Modifier::empty(), Vec::new()),
            node(
                3,
                Modifier::empty().semantics(|config| config.is_modal = true),
                vec![
                    node(
                        4,
                        Modifier::empty().selectable_group(),
                        vec![
                            tab(5, false, true),
                            node(
                                6,
                                Modifier::empty().selectable_group(),
                                vec![tab(7, true, true), tab(8, false, true)],
                            ),
                            tab(9, true, true),
                        ],
                    ),
                    node(
                        10,
                        Modifier::empty().hide_from_accessibility(),
                        vec![node(11, Modifier::empty(), Vec::new())],
                    ),
                ],
            ),
        ],
    ));
    assert_eq!(
        keyboard_focus_target(&tree, None, KeyCode::Tab, false)
            .unwrap()
            .node_id,
        7
    );
    assert_eq!(
        keyboard_focus_target(&tree, Some(7), KeyCode::End, false)
            .unwrap()
            .node_id,
        8
    );
    assert_eq!(
        keyboard_focus_target(&tree, Some(9), KeyCode::ArrowLeft, false)
            .unwrap()
            .node_id,
        5
    );
    assert_eq!(
        keyboard_focus_target(&tree, Some(9), KeyCode::Tab, false)
            .unwrap()
            .node_id,
        7
    );
}

#[test]
fn keyboard_navigation_publishes_the_order_for_programmatic_focus() {
    let _scope = crate::render_state::app_context_test_scope();
    crate::set_focus_order(Vec::new());
    let tree = LayoutTree::new(node(
        1,
        Modifier::empty(),
        vec![
            node(2, Modifier::empty(), Vec::new()),
            node(3, Modifier::empty(), Vec::new()),
        ],
    ));
    assert!(keyboard_focus_target(&tree, None, KeyCode::Tab, false).is_some());
    assert_eq!(crate::focus_order_len(), 2);
    assert!(crate::FocusManager.move_focus(crate::FocusDirection::Next));
    assert_eq!(crate::active_focus_target(), Some(2));
}

#[test]
fn focus_order_is_owned_by_its_application() {
    let first = crate::render_state::AppContext::new();
    let second = crate::render_state::AppContext::new();
    let entry = FocusEntry {
        node_id: 1,
        rect: Rect::from_size(Size::new(10.0, 10.0)),
    };
    first.enter(|| crate::set_focus_order(vec![entry]));
    second.enter(|| {
        assert_eq!(crate::focus_order_len(), 0);
        crate::set_focus_order(Vec::new());
    });
    first.enter(|| assert_eq!(crate::focus_order_len(), 1));
}
