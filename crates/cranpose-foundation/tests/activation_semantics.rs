use std::{cell::Cell, rc::Rc};

use cranpose_foundation::SemanticsConfiguration;

#[test]
fn explicit_activation_is_advertised_and_invokes_its_callback() {
    let count = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&count);
    let config = SemanticsConfiguration::new().on_click("Open document", move || {
        recorded.set(recorded.get() + 1);
    });
    assert!(config.is_activatable());
    assert!(!config.is_clickable);
    let action = config.on_click.expect("activation callback");
    assert_eq!(action.label, "Open document");
    action.invoke();
    assert_eq!(count.get(), 1);
}

#[test]
fn merging_activation_replaces_the_callback_and_preserves_other_semantics() {
    let count = Rc::new(Cell::new(0));
    let recorded = Rc::clone(&count);
    let mut config = SemanticsConfiguration::new()
        .content_description("Receipt")
        .on_click("Old action", || panic!("replaced callback"));
    config.merge(
        &SemanticsConfiguration::new().on_click("Edit receipt", move || {
            recorded.set(recorded.get() + 1);
        }),
    );
    config.merge(&SemanticsConfiguration::new());
    assert_eq!(config.content_description.as_deref(), Some("Receipt"));
    assert!(config.is_activatable());
    let action = config.on_click.expect("merged activation");
    assert_eq!(action.label, "Edit receipt");
    action.invoke();
    assert_eq!(count.get(), 1);
}
