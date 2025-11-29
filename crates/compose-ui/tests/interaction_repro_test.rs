use compose_ui::Modifier;
use compose_testing::{ComposeTestRule, has_text};
use compose_ui::{Column, ColumnSpec, Text};
use compose_ui::{ScrollState, Size};
use compose_core::{useState, NodeId};
use compose_foundation::PointerEventKind;
use std::rc::Rc;

#[test]
fn scroll_reactivity_repro() {
    let mut rule = ComposeTestRule::new();

    let scroll_state_capture = Rc::new(std::cell::RefCell::new(None));
    let capture = scroll_state_capture.clone();

    rule.set_content(move || {
        let scroll_state_state = useState(|| ScrollState::new(0));
        let scroll_state = scroll_state_state.get();
        *capture.borrow_mut() = Some(scroll_state.clone());

        Column(
            Modifier::empty()
                .size(Size { width: 100.0, height: 100.0 })
                .vertical_scroll(scroll_state),
            ColumnSpec::default(),
            || {
                // Content larger than 100.0 to enable scrolling
                Text("Item 1", Modifier::empty().size(Size { width: 100.0, height: 50.0 }));
                Text("Item 2", Modifier::empty().size(Size { width: 100.0, height: 50.0 }));
                Text("Item 3", Modifier::empty().size(Size { width: 100.0, height: 50.0 }));
            },
        );
    });

    let scroll_state = scroll_state_capture.borrow().as_ref().unwrap().clone();
    
    // Initial state
    assert_eq!(scroll_state.value(), 0);

    // Perform scroll gesture (drag up) using TestNode API
    // Find "Item 1" which is inside the scroll container
    rule.on_node(has_text("Item 1"))
        .perform_touch_input(|touch| {
            touch.swipe_up(40.0);
        });

    // Verify scroll state changed
    let value = scroll_state.value();
    println!("Scroll value after gesture: {:?}", value);
    assert!(value > 0, "Scroll value should increase after drag up");
}
