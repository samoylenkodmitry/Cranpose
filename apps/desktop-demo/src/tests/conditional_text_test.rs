use compose_core::{self};
use compose_ui::{composable, Column, ColumnSpec, Modifier, Text};

#[composable]
fn conditional_text_with_external_state(counter_state: compose_core::MutableState<i32>) {
    // Mimic the exact pattern from counter_app - with_key BEFORE the Column
    let is_even = counter_state.get() % 2 == 0;
    compose_core::with_key(&is_even, || {
        if is_even {
            Text("if counter % 2 == 0", Modifier::empty());
        } else {
            Text("if counter % 2 != 0", Modifier::empty());
        }
    });

    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(
            format!("Counter: {}", counter_state.get()),
            Modifier::empty(),
        );
    });
}

#[test]
fn test_conditional_text_reactivity() {
    use compose_core::{MutableState, NodeError};
    use compose_ui::run_test_composition;
    use std::cell::RefCell;

    thread_local! {
        static TEST_COUNTER: RefCell<Option<MutableState<i32>>> = RefCell::new(None);
    }

    // Helper function to drain recompositions
    fn drain_all(composition: &mut compose_ui::TestComposition) -> Result<(), NodeError> {
        loop {
            if !composition.process_invalid_scopes()? {
                break;
            }
        }
        Ok(())
    }

    // Initial composition - counter is 0 (even)
    let mut composition = run_test_composition(|| {
        let counter = compose_core::useState(|| 0);
        TEST_COUNTER.with(|cell| {
            *cell.borrow_mut() = Some(counter.clone());
        });
        conditional_text_with_external_state(counter);
    });

    let tree = composition.applier_mut().dump_tree(Some(0));
    println!("\n=== Initial composition (counter=0) ===\n{}", tree);
    let initial_node_count = tree.lines().count();
    println!("Initial node count: {}", initial_node_count);

    // Get the counter state and increment it
    let counter = TEST_COUNTER.with(|cell| cell.borrow().clone().expect("counter state not set"));

    // Increment the counter to 1 (odd)
    counter.set(1);
    drain_all(&mut composition).expect("drain after increment to 1");

    let tree = composition.applier_mut().dump_tree(Some(0));
    println!("\n=== After incrementing to 1 ===\n{}", tree);
    let after_1_node_count = tree.lines().count();
    println!("After increment to 1, node count: {}", after_1_node_count);

    // The node count should stay the same - we're replacing one Text node with another
    assert_eq!(
        initial_node_count, after_1_node_count,
        "Node count should remain the same when counter changes from 0 to 1"
    );

    // Increment again to 2 (even)
    counter.set(2);
    drain_all(&mut composition).expect("drain after increment to 2");

    let tree = composition.applier_mut().dump_tree(Some(0));
    println!("\n=== After incrementing to 2 ===\n{}", tree);
    let after_2_node_count = tree.lines().count();
    println!("After increment to 2, node count: {}", after_2_node_count);

    assert_eq!(
        initial_node_count, after_2_node_count,
        "Node count should remain the same when counter changes from 1 to 2"
    );
}
