use super::*;

#[derive(Default)]
struct TestNode {
    value: i32,
}

impl Node for TestNode {}

#[test]
fn compose_test_rule_reports_content_and_root() {
    run_test_composition(|rule| {
        assert!(!rule.has_content());
        assert!(rule.root_id().is_none());

        let runtime = rule.runtime_handle();
        let state = MutableState::with_runtime(0, runtime.clone());
        let recompositions = Rc::new(Cell::new(0));

        rule.set_content({
            let recompositions = Rc::clone(&recompositions);
            move || {
                recompositions.set(recompositions.get() + 1);
                let id = with_current_composer(|composer| composer.emit_node(TestNode::default));
                let value = state.value();
                with_node_mut(id, |node: &mut TestNode| {
                    node.value = value;
                })
                .expect("update node text");
            }
        })
        .expect("install content");

        assert!(rule.has_content());
        assert_eq!(recompositions.get(), 1);

        let root = rule.root_id().expect("root id available");
        let stored_value = {
            rule.applier_mut()
                .with_node(root, |node: &mut TestNode| node.value)
                .expect("read node")
        };
        assert_eq!(stored_value, 0);

        state.set_value(5);
        rule.pump_until_idle().expect("process invalidation");

        assert_eq!(recompositions.get(), 2);
        let updated_value = {
            rule.applier_mut()
                .with_node(root, |node: &mut TestNode| node.value)
                .expect("read updated node")
        };
        assert_eq!(updated_value, 5);
    });
}
