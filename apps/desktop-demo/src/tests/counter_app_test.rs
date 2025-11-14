use super::*;
use compose_core::{location_key, Applier, Composition, MemoryApplier, NodeError};
use compose_ui::widgets::nodes::{ButtonNode, TextNode};

fn drain_all(composition: &mut Composition<MemoryApplier>) -> Result<(), NodeError> {
    loop {
        if !composition.process_invalid_scopes()? {
            break;
        }
    }
    Ok(())
}

/// Helper to find all TextNodes and return their text content
fn collect_text_content(applier: &mut impl Applier) -> Vec<String> {
    let mut texts = Vec::new();

    // Iterate through all nodes in the applier
    for i in 0..1000 {  // reasonable upper bound
        if let Ok(node) = applier.get_mut(i) {
            if let Some(text_node) = node.as_any_mut().downcast_mut::<TextNode>() {
                texts.push(text_node.text.clone());
            }
        }
    }

    texts
}

/// Helper to find the first ButtonNode with "Increment" text in its children and invoke its callback
fn click_increment_button(applier: &mut impl Applier) -> bool {
    // Search for ButtonNode
    for i in 0..1000 {
        if let Ok(node) = applier.get_mut(i) {
            if let Some(button_node) = node.as_any_mut().downcast_mut::<ButtonNode>() {
                let on_click = button_node.on_click.clone();
                let children = button_node.children.clone();

                // Check if this button's children contain "Increment" text
                for child_id in children {
                    if let Ok(child_node) = applier.get_mut(child_id) {
                        if let Some(text_node) = child_node.as_any_mut().downcast_mut::<TextNode>() {
                            if text_node.text.contains("Increment") {
                                // Found the increment button! Click it
                                (on_click.borrow_mut())();
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    false
}

#[test]
fn counter_app_conditional_text_updates_on_increment() {
    let mut composition = Composition::new(MemoryApplier::new());

    let mut render = move || {
        counter_app();
    };

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("initial render");
    drain_all(&mut composition).expect("initial drain");

    // Check initial state: counter is 0 (even)
    let texts_initial = collect_text_content(&mut *composition.applier_mut());
    println!("\n=== Initial texts ===");
    for (i, text) in texts_initial.iter().enumerate() {
        println!("  [{}]: {}", i, text);
    }

    // Should contain "if counter % 2 == 0" when counter is 0
    assert!(
        texts_initial.iter().any(|t| t.contains("if counter % 2 == 0")),
        "Expected to find 'if counter % 2 == 0' text when counter is 0"
    );
    assert!(
        !texts_initial.iter().any(|t| t.contains("if counter % 2 != 0")),
        "Should NOT find 'if counter % 2 != 0' text when counter is 0"
    );

    // Simulate clicking the increment button
    assert!(click_increment_button(&mut *composition.applier_mut()), "Failed to find and click increment button");

    // Recompose
    drain_all(&mut composition).expect("drain after increment");

    // Check state after increment: counter is 1 (odd)
    let texts_after_increment = collect_text_content(&mut *composition.applier_mut());
    println!("\n=== After increment to 1 ===");
    for (i, text) in texts_after_increment.iter().enumerate() {
        println!("  [{}]: {}", i, text);
    }

    // Should now contain "if counter % 2 != 0" when counter is 1
    assert!(
        texts_after_increment.iter().any(|t| t.contains("if counter % 2 != 0")),
        "Expected to find 'if counter % 2 != 0' text when counter is 1"
    );
    assert!(
        !texts_after_increment.iter().any(|t| t.contains("if counter % 2 == 0")),
        "Should NOT find 'if counter % 2 == 0' text when counter is 1 (old node should be removed)"
    );

    // Click increment again: counter becomes 2 (even)
    assert!(click_increment_button(&mut *composition.applier_mut()), "Failed to find and click increment button second time");
    drain_all(&mut composition).expect("drain after second increment");

    let texts_after_second_increment = collect_text_content(&mut *composition.applier_mut());
    println!("\n=== After increment to 2 ===");
    for (i, text) in texts_after_second_increment.iter().enumerate() {
        println!("  [{}]: {}", i, text);
    }

    // Should be back to "if counter % 2 == 0" when counter is 2
    assert!(
        texts_after_second_increment.iter().any(|t| t.contains("if counter % 2 == 0")),
        "Expected to find 'if counter % 2 == 0' text when counter is 2"
    );
    assert!(
        !texts_after_second_increment.iter().any(|t| t.contains("if counter % 2 != 0")),
        "Should NOT find 'if counter % 2 != 0' text when counter is 2"
    );
}
