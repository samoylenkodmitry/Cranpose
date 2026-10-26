use cranpose_core::NodeId;

use crate::{
    clear_focus_invalidations, clear_pointer_repasses, clear_semantics_invalidations,
    has_pending_focus_invalidations, has_pending_pointer_repasses,
    has_pending_semantics_invalidations, process_focus_invalidations, process_pointer_repasses,
    process_semantics_invalidations,
    render_state::{AppContext, app_context_test_scope},
    schedule_focus_invalidation, schedule_pointer_repass, schedule_semantics_invalidation,
};

struct Queue {
    name: &'static str,
    schedule: fn(NodeId),
    has_pending: fn() -> bool,
    process: fn(&mut dyn FnMut(NodeId)),
    clear: fn(),
}

impl Queue {
    fn take(&self) -> Vec<NodeId> {
        let mut processed = Vec::new();
        (self.process)(&mut |node_id| processed.push(node_id));
        processed.sort_unstable();
        processed
    }
}

const QUEUES: [Queue; 3] = [
    Queue {
        name: "pointer",
        schedule: schedule_pointer_repass,
        has_pending: has_pending_pointer_repasses,
        process: |processor| process_pointer_repasses(processor),
        clear: clear_pointer_repasses,
    },
    Queue {
        name: "focus",
        schedule: schedule_focus_invalidation,
        has_pending: has_pending_focus_invalidations,
        process: |processor| process_focus_invalidations(processor),
        clear: clear_focus_invalidations,
    },
    Queue {
        name: "semantics",
        schedule: schedule_semantics_invalidation,
        has_pending: has_pending_semantics_invalidations,
        process: |processor| process_semantics_invalidations(processor),
        clear: clear_semantics_invalidations,
    },
];

#[test]
fn a_scheduled_node_reaches_the_processor_once() {
    let _app_context = app_context_test_scope();
    for queue in &QUEUES {
        (queue.clear)();
        assert!(!(queue.has_pending)(), "{} starts empty", queue.name);
        assert!(
            queue.take().is_empty(),
            "{} has nothing to hand",
            queue.name
        );

        (queue.schedule)(NodeId(7));
        (queue.schedule)(NodeId(7));
        (queue.schedule)(NodeId(9));
        assert!((queue.has_pending)(), "{} holds the nodes", queue.name);
        assert_eq!(
            queue.take(),
            vec![NodeId(7), NodeId(9)],
            "{} hands each node once",
            queue.name
        );
        assert!(
            !(queue.has_pending)(),
            "{} is empty after a pass",
            queue.name
        );
    }
}

#[test]
fn a_cleared_queue_hands_nothing() {
    let _app_context = app_context_test_scope();
    for queue in &QUEUES {
        (queue.schedule)(NodeId(3));
        (queue.clear)();
        assert!(
            !(queue.has_pending)(),
            "{} is empty after clear",
            queue.name
        );
        assert!(
            queue.take().is_empty(),
            "{} hands nothing after clear",
            queue.name
        );
    }
}

#[test]
fn nodes_a_panicking_processor_did_not_reach_stay_queued() {
    let _app_context = app_context_test_scope();
    for queue in &QUEUES {
        (queue.clear)();
        (queue.schedule)(NodeId(1));
        (queue.schedule)(NodeId(2));
        let mut first = None;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (queue.process)(&mut |node_id| {
                first = Some(node_id);
                panic!("{} processor panic", queue.name);
            });
        }));
        assert!(result.is_err(), "{} passes the panic on", queue.name);

        let unreached = if first == Some(NodeId(1)) {
            NodeId(2)
        } else {
            NodeId(1)
        };
        assert_eq!(
            queue.take(),
            vec![unreached],
            "{} keeps the node the processor did not reach and runs again",
            queue.name
        );
        assert!(!(queue.has_pending)(), "{} is empty again", queue.name);
    }
}

#[test]
fn a_processor_schedules_work_for_the_next_pass() {
    let _app_context = app_context_test_scope();
    for queue in &QUEUES {
        (queue.clear)();
        (queue.schedule)(NodeId(1));
        let mut nested = Vec::new();
        (queue.process)(&mut |_| {
            (queue.schedule)(NodeId(2));
            (queue.process)(&mut |node_id| nested.push(node_id));
        });
        assert!(
            nested.is_empty(),
            "{} runs no pass inside a running pass",
            queue.name
        );
        assert!((queue.has_pending)(), "{} holds the new node", queue.name);
        assert_eq!(
            queue.take(),
            vec![NodeId(2)],
            "{} hands it next pass",
            queue.name
        );
    }
}

#[test]
fn each_app_context_has_its_own_queues() {
    let _app_context = app_context_test_scope();
    let first = AppContext::new_with_density(1.0);
    let second = AppContext::new_with_density(1.0);
    for queue in &QUEUES {
        first.enter(|| {
            (queue.clear)();
            (queue.schedule)(NodeId(7));
        });
        second.enter(|| {
            (queue.clear)();
            assert!(
                !(queue.has_pending)(),
                "{} of another context is empty",
                queue.name
            );
            (queue.schedule)(NodeId(9));
        });
        first.enter(|| assert_eq!(queue.take(), vec![NodeId(7)], "{} of the first", queue.name));
        second.enter(|| {
            assert_eq!(
                queue.take(),
                vec![NodeId(9)],
                "{} of the second",
                queue.name
            );
        });
    }
}
