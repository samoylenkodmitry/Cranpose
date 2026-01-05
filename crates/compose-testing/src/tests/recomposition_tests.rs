use super::*;
use compose_macros::composable;

#[derive(Default)]
struct TestContainerNode {
    parent: Option<NodeId>,
}

impl Node for TestContainerNode {
    fn on_attached_to_parent(&mut self, parent: NodeId) {
        self.parent = Some(parent);
    }
    fn on_removed_from_parent(&mut self) {
        self.parent = None;
    }
    fn parent(&self) -> Option<NodeId> {
        self.parent
    }
}

#[derive(Default)]
struct TestTextNode {
    content: String,
    parent: Option<NodeId>,
}

impl Node for TestTextNode {
    fn on_attached_to_parent(&mut self, parent: NodeId) {
        self.parent = Some(parent);
    }
    fn on_removed_from_parent(&mut self) {
        self.parent = None;
    }
    fn parent(&self) -> Option<NodeId> {
        self.parent
    }
}

#[allow(non_snake_case)]
#[composable]
fn Column(content: impl FnOnce()) {
    let id =
        with_current_composer(|composer| composer.emit_node(|| TestContainerNode { parent: None }));
    push_parent(id);
    content();
    pop_parent();
}

#[allow(non_snake_case)]
#[composable]
fn Text(value: String) {
    let initial_content = value.clone();
    let id = with_current_composer(|composer| {
        composer.emit_node(|| TestTextNode {
            content: initial_content,
            parent: None,
        })
    });
    with_node_mut(id, |node: &mut TestTextNode| {
        node.content = value;
    })
    .expect("update text node");
}

#[allow(non_snake_case)]
#[composable]
fn Parent(value: i32) {
    Column(|| {
        Child(value);
    });
}

#[allow(non_snake_case)]
#[composable]
fn Child(value: i32) {
    Text(format!("value: {}", value));
}

#[test]
fn test_child_recomposition_preserves_parent() {
    run_test_composition(|rule| {
        let runtime = rule.runtime_handle();
        let text_state = MutableState::with_runtime("Hello".to_string(), runtime.clone());

        rule.set_content({
            move || {
                Column(|| {
                    let value = text_state.value();
                    Text(value);
                });
            }
        })
        .expect("initial render succeeds");

        assert_eq!(rule.applier_mut().len(), 2);

        text_state.set_value("World".to_string());
        {
            let composition = rule.composition();
            let _ = composition
                .process_invalid_scopes()
                .expect("process invalid scopes");
        }

        assert_eq!(rule.applier_mut().len(), 2);
    });
}

#[test]
fn test_conditional_composable_preserves_siblings() {
    run_test_composition(|rule| {
        let runtime = rule.runtime_handle();
        let show_middle = MutableState::with_runtime(true, runtime.clone());

        rule.set_content({
            move || {
                Column(|| {
                    Text("A".to_string());
                    if show_middle.value() {
                        Text("B".to_string());
                    }
                    Text("C".to_string());
                });
            }
        })
        .expect("initial render succeeds");

        assert_eq!(rule.applier_mut().len(), 4);

        show_middle.set_value(false);
        rule.recomposition()
            .expect("second render with middle hidden");
        rule.pump_until_idle()
            .expect("drain pending work after hiding middle");
        assert_eq!(rule.applier_mut().len(), 3);

        show_middle.set_value(true);
        rule.recomposition()
            .expect("third render with middle visible");
        rule.pump_until_idle()
            .expect("drain pending work after showing middle");
        assert_eq!(rule.applier_mut().len(), 4);
    });
}

#[test]
fn test_skipped_composable_preserves_children() {
    run_test_composition(|rule| {
        rule.set_content(|| {
            Parent(1);
        })
        .expect("initial render succeeds");

        assert_eq!(rule.applier_mut().len(), 2);

        rule.recomposition().expect("recompose with stable input");
        assert_eq!(rule.applier_mut().len(), 2);
    });
}
