use super::*;
use cranpose_core::rememberMutableStateOf;

thread_local! {
    static ROOT_RENDER_TEST_INVALIDATED: Cell<bool> = const { Cell::new(false) };
}

#[derive(Default)]
struct TestNode {
    value: i32,
}

impl Node for TestNode {}

#[derive(Default)]
struct TestTextValueNode {
    value: String,
}

impl Node for TestTextValueNode {}

#[test]
fn cranpose_test_rule_reports_content_and_root() {
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

#[test]
fn cranpose_test_rule_replays_root_render_requests_before_returning() {
    run_test_composition(|rule| {
        ROOT_RENDER_TEST_INVALIDATED.with(|flag| flag.set(false));
        let render_count = Rc::new(Cell::new(0));

        rule.set_content({
            let render_count = Rc::clone(&render_count);
            move || {
                let root_trigger = rememberMutableStateOf(|| false);
                render_count.set(render_count.get() + 1);
                cranpose_core::with_key(&"testing-root-render-probe", || {
                    let _ = root_trigger.value();
                });

                let value = format!("render {}", render_count.get());
                let initial_value = value.clone();
                let id = with_current_composer(|composer| {
                    composer.emit_node(|| TestTextValueNode {
                        value: initial_value,
                    })
                });
                with_node_mut(id, |node: &mut TestTextValueNode| {
                    node.value = value;
                })
                .expect("update test text value node");

                cranpose_core::SideEffect(move || {
                    let already_invalidated =
                        ROOT_RENDER_TEST_INVALIDATED.with(|flag| flag.replace(true));
                    if already_invalidated {
                        return;
                    }
                    root_trigger.set_value(true);
                });
            }
        })
        .expect("install content");

        assert_eq!(
            render_count.get(),
            2,
            "ComposeTestRule::set_content must replay pending root renders before returning"
        );
        assert!(
            !rule.composition().take_root_render_request(),
            "test harness should not leave a pending root render request behind"
        );

        let root = rule.root_id().expect("root id available");
        let stored_value = rule
            .applier_mut()
            .with_node(root, |node: &mut TestTextValueNode| node.value.clone())
            .expect("read test node");
        assert_eq!(stored_value, "render 2");
    });
}

#[test]
fn cranpose_test_rule_reports_idle_replay_limit_without_panicking() {
    run_test_composition(|rule| {
        rule.composition().request_root_render();

        let result = rule.pump_until_idle();

        assert_eq!(
            result,
            Err(NodeError::RecompositionLimitExceeded {
                operation: "pump_until_idle",
                limit: ROOT_RENDER_REPLAY_LIMIT,
            })
        );
    });
}

#[test]
fn cranpose_test_rule_reports_root_render_replay_limit_without_panicking() {
    run_test_composition(|rule| {
        let result = rule.set_content(|| {
            let value = rememberMutableStateOf(|| 0usize);
            let current = value.value();
            cranpose_core::SideEffect(move || {
                value.set_value(current + 1);
            });
        });

        assert_eq!(
            result,
            Err(NodeError::RecompositionLimitExceeded {
                operation: "root render replay",
                limit: ROOT_RENDER_REPLAY_LIMIT,
            })
        );
    });
}
