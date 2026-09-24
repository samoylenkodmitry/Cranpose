use cranpose_foundation::{BasicModifierNodeContext, ModifierNodeChain};

use super::*;

#[test]
fn focus_target_node_lifecycle() {
    let mut node = FocusTargetNode::new();
    let mut context = BasicModifierNodeContext::new();

    assert_eq!(node.focus_state(), FocusState::Inactive);
    assert!(!node.node_state().is_attached());

    node.on_attach(&mut context);
    assert!(node.node_state().is_attached());

    node.set_focus_state(FocusState::Active);
    assert_eq!(node.focus_state(), FocusState::Active);
    assert!(node.focus_state().is_focused());

    node.on_detach();
    assert!(!node.node_state().is_attached());
    assert_eq!(node.focus_state(), FocusState::Inactive);
}

#[test]
fn focus_target_callback_invoked() {
    use std::cell::RefCell;
    let states = Rc::new(RefCell::new(Vec::new()));
    let states_clone = states.clone();

    let node = FocusTargetNode::with_callback(move |state| {
        states_clone.borrow_mut().push(state);
    });

    node.set_focus_state(FocusState::Active);
    node.set_focus_state(FocusState::ActiveParent);
    node.set_focus_state(FocusState::Inactive);

    let recorded = states.borrow();
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[0], FocusState::Active);
    assert_eq!(recorded[1], FocusState::ActiveParent);
    assert_eq!(recorded[2], FocusState::Inactive);
}

#[test]
fn focus_element_creates_node() {
    let element = FocusTargetElement::new();
    let node = element.create();
    assert_eq!(node.focus_state(), FocusState::Inactive);
}

#[test]
fn focus_chain_integration() {
    let element = FocusTargetElement::new();
    let dyn_element = cranpose_foundation::modifier_element(element);

    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    chain.update(vec![dyn_element], &mut context);

    assert_eq!(chain.len(), 1);
    assert!(chain.has_capability(NodeCapabilities::FOCUS));
}

#[test]
fn focus_state_predicates() {
    assert!(FocusState::Active.is_focused());
    assert!(FocusState::Captured.is_focused());
    assert!(!FocusState::Inactive.is_focused());
    assert!(!FocusState::ActiveParent.is_focused());

    assert!(FocusState::Active.has_focus());
    assert!(FocusState::ActiveParent.has_focus());
    assert!(FocusState::Captured.has_focus());
    assert!(!FocusState::Inactive.has_focus());

    assert!(FocusState::Captured.is_captured());
    assert!(!FocusState::Active.is_captured());
}

fn attach_at(
    node_id: NodeId,
    elements: Vec<cranpose_foundation::DynModifierElement>,
) -> ModifierNodeChain {
    let mut context = BasicModifierNodeContext::new();
    context.set_node_id(Some(node_id));
    let mut chain = ModifierNodeChain::new();
    chain.update(elements, &mut context);
    chain
}

#[test]
fn request_focus_on_a_never_attached_requester_fails_predictably() {
    let requester = FocusRequester::new();
    assert_eq!(
        requester.request_focus(),
        Err(FocusRequestError::NotAttached)
    );
}

#[test]
fn request_focus_on_a_requester_with_no_focus_target_fails_predictably() {
    let _app_context = crate::render_state::app_context_test_scope();
    let requester = FocusRequester::new();

    let _chain = attach_at(
        1,
        vec![cranpose_foundation::modifier_element(
            FocusRequesterElement::new(requester.clone()),
        )],
    );

    assert_eq!(requester.node_id(), Some(1));
    assert_eq!(
        requester.request_focus(),
        Err(FocusRequestError::NoFocusTarget)
    );
}

#[test]
fn a_focus_requester_moves_focus_onto_its_paired_focus_target() {
    let _app_context = crate::render_state::app_context_test_scope();
    let requester = FocusRequester::new();

    let chain = attach_at(
        2,
        vec![
            cranpose_foundation::modifier_element(FocusRequesterElement::new(requester.clone())),
            cranpose_foundation::modifier_element(FocusTargetElement::new()),
        ],
    );

    assert_eq!(requester.request_focus(), Ok(()));

    let target = chain.node::<FocusTargetNode>(1).expect("focus target node");
    assert_eq!(target.focus_state(), FocusState::Active);
    assert_eq!(focus_dispatch::active_focus_target(), Some(2));
}

#[test]
fn request_focus_after_the_node_leaves_composition_fails_predictably() {
    let _app_context = crate::render_state::app_context_test_scope();
    let requester = FocusRequester::new();

    let mut context = BasicModifierNodeContext::new();
    context.set_node_id(Some(3));
    let mut chain = ModifierNodeChain::new();
    chain.update(
        vec![
            cranpose_foundation::modifier_element(FocusRequesterElement::new(requester.clone())),
            cranpose_foundation::modifier_element(FocusTargetElement::new()),
        ],
        &mut context,
    );
    assert!(requester.request_focus().is_ok());

    chain.update(Vec::new(), &mut context);

    assert_eq!(
        requester.request_focus(),
        Err(FocusRequestError::NotAttached)
    );
}

#[test]
fn focus_survives_the_requested_node_being_recomposed() {
    let _app_context = crate::render_state::app_context_test_scope();
    let requester = FocusRequester::new();

    let mut context = BasicModifierNodeContext::new();
    context.set_node_id(Some(4));
    let mut chain = ModifierNodeChain::new();
    let make_elements = || {
        vec![
            cranpose_foundation::modifier_element(FocusRequesterElement::new(requester.clone())),
            cranpose_foundation::modifier_element(FocusTargetElement::new()),
        ]
    };
    chain.update(make_elements(), &mut context);
    assert_eq!(requester.request_focus(), Ok(()));

    let node_ptr_before = {
        let node = chain.node::<FocusTargetNode>(1).unwrap();
        &*node as *const FocusTargetNode
    };

    chain.update(make_elements(), &mut context);

    let target = chain.node::<FocusTargetNode>(1).unwrap();
    let node_ptr_after = &*target as *const FocusTargetNode;
    assert_eq!(
        node_ptr_before, node_ptr_after,
        "recomposition with a structurally equal modifier must reuse the node"
    );
    assert_eq!(
        target.focus_state(),
        FocusState::Active,
        "recomposing the focused node must not reset its focus state"
    );
    assert_eq!(focus_dispatch::active_focus_target(), Some(4));
}

#[test]
fn requesting_focus_from_inside_on_focus_changed_does_not_double_borrow_or_recurse() {
    let _app_context = crate::render_state::app_context_test_scope();

    let requester_a = FocusRequester::new();
    let requester_b = FocusRequester::new();
    let requester_b_for_callback = requester_b.clone();
    let bounced = Rc::new(Cell::new(false));
    let bounced_for_callback = bounced.clone();

    let chain_a = attach_at(
        10,
        vec![
            cranpose_foundation::modifier_element(FocusRequesterElement::new(requester_a.clone())),
            cranpose_foundation::modifier_element(FocusTargetElement::with_callback(
                move |state| {
                    if state == FocusState::Active && !bounced_for_callback.get() {
                        bounced_for_callback.set(true);
                        requester_b_for_callback
                            .request_focus()
                            .expect("a reentrant request_focus must succeed, not double-borrow");
                    }
                },
            )),
        ],
    );
    let chain_b = attach_at(
        20,
        vec![
            cranpose_foundation::modifier_element(FocusRequesterElement::new(requester_b)),
            cranpose_foundation::modifier_element(FocusTargetElement::new()),
        ],
    );

    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| requester_a.request_focus()));
    assert!(result.is_ok(), "a reentrant focus request must not panic");
    assert_eq!(result.unwrap(), Ok(()));
    assert!(bounced.get(), "the reentrant callback never ran");

    let target_a = chain_a.node::<FocusTargetNode>(1).unwrap();
    let target_b = chain_b.node::<FocusTargetNode>(1).unwrap();
    assert_eq!(target_a.focus_state(), FocusState::Inactive);
    assert_eq!(target_b.focus_state(), FocusState::Active);
    assert_eq!(focus_dispatch::active_focus_target(), Some(20));
}
