use compose_core::{self};
use compose_ui::{composable, Text, Modifier, Column, ColumnSpec};

#[composable]
fn conditional_text_with_external_state(counter_state: compose_core::MutableState<i32>) {
    println!("===composable called===");
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        println!("===column content called===");
        let is_even = counter_state.get() % 2 == 0;
        println!("is_even = {}", is_even);
        compose_core::with_key(&is_even, || {
            if is_even {
                println!("rendering 'if counter % 2 == 0'");
                Text("if counter % 2 == 0", Modifier::empty());
            } else {
                println!("rendering 'if counter % 2 != 0'");
                Text("if counter % 2 != 0", Modifier::empty());
            }
        });

        let count = counter_state.get();
        println!("rendering 'Counter: {}'", count);
        Text(format!("Counter: {}", count), Modifier::empty());
    });
}

#[test]
fn test_conditional_text_reactivity() {
    use compose_ui::run_test_composition;
    use compose_core::{MutableState, NodeError};
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
    assert!(tree.contains("if counter % 2 == 0"),
            "Initial: Should show 'if counter % 2 == 0' when counter is 0");
    assert!(tree.contains("Counter: 0"),
            "Initial: Should show 'Counter: 0'");

    // Get the counter state and increment it
    let counter = TEST_COUNTER.with(|cell| {
        cell.borrow().clone().expect("counter state not set")
    });

    // Increment the counter to 1
    counter.set(1);
    drain_all(&mut composition).expect("drain after increment to 1");

    let tree = composition.applier_mut().dump_tree(Some(0));
    println!("\n=== After incrementing to 1 ===\n{}", tree);
    assert!(tree.contains("Counter: 1"),
            "After increment: Should show 'Counter: 1'");
    assert!(tree.contains("if counter % 2 != 0"),
            "After increment: Should show 'if counter % 2 != 0' when counter is 1");
    assert!(!tree.contains("if counter % 2 == 0"),
            "After increment: Should NOT show 'if counter % 2 == 0' when counter is 1");

    // Increment again to 2 (even)
    counter.set(2);
    drain_all(&mut composition).expect("drain after increment to 2");

    let tree = composition.applier_mut().dump_tree(Some(0));
    println!("\n=== After incrementing to 2 ===\n{}", tree);
    assert!(tree.contains("Counter: 2"),
            "After second increment: Should show 'Counter: 2'");
    assert!(tree.contains("if counter % 2 == 0"),
            "After second increment: Should show 'if counter % 2 == 0' when counter is 2");
    assert!(!tree.contains("if counter % 2 != 0"),
            "After second increment: Should NOT show 'if counter % 2 != 0' when counter is 2");
}
