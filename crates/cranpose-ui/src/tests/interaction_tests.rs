use cranpose_core::{Composition, MemoryApplier};

use super::*;

#[test]
fn interaction_ids_do_not_use_process_global_counters() {
    let source = include_str!("../interaction.rs");
    let source_counter = ["static ", "NEXT_SOURCE_ID"].concat();
    let press_counter = ["static ", "NEXT_PRESS_ID"].concat();

    assert!(
        !source.contains(&source_counter) && !source.contains(&press_counter),
        "interaction source and press ids must be owned by the interaction source instance"
    );
}

#[test]
fn interaction_source_tracks_active_press_state() {
    let composition = Composition::new(MemoryApplier::new());
    let source = MutableInteractionSource::with_runtime(composition.runtime_handle());
    let pressed = source.collectIsPressedAsState();

    assert!(!pressed.get());

    let first = source.press(Point { x: 1.0, y: 2.0 });
    assert!(pressed.get());

    let second = source.press(Point { x: 3.0, y: 4.0 });
    assert_ne!(first.id(), second.id());
    source.release(first);
    assert!(pressed.get());

    source.cancel(second);
    assert!(!pressed.get());
}

#[test]
fn interaction_source_ids_are_instance_owned() {
    let composition = Composition::new(MemoryApplier::new());
    let first = MutableInteractionSource::with_runtime(composition.runtime_handle());
    let first_clone = first;
    let second = MutableInteractionSource::with_runtime(composition.runtime_handle());

    assert_eq!(first.id(), first_clone.id());
    assert_ne!(first.id(), second.id());
    assert_eq!(first.press(Point { x: 0.0, y: 0.0 }).id(), 1);
    assert_eq!(first.press(Point { x: 1.0, y: 1.0 }).id(), 2);
    assert_eq!(second.press(Point { x: 0.0, y: 0.0 }).id(), 1);
}

#[test]
fn collect_is_pressed_as_state_tracks_press_emissions() {
    let composition = Composition::new(MemoryApplier::new());
    let source = MutableInteractionSource::with_runtime(composition.runtime_handle());
    let pressed = collect_is_pressed_as_state(&source);

    assert!(!pressed.get());

    let press = PressInteractionPress {
        id: 7,
        press_position: Point { x: 4.0, y: 5.0 },
    };
    source.emit(Interaction::Press(PressInteraction::Press(press)));
    assert!(pressed.get(), "pressed after a Press emission");

    source.emit(Interaction::Press(PressInteraction::Release(
        PressInteractionRelease { press },
    )));
    assert!(!pressed.get(), "released after a Release emission");

    source.emit(Interaction::Press(PressInteraction::Press(press)));
    assert!(pressed.get(), "pressed again after a new Press emission");

    source.emit(Interaction::Press(PressInteraction::Cancel(
        PressInteractionCancel { press },
    )));
    assert!(!pressed.get(), "released after a Cancel emission");
}

#[composable]
fn PressedReader(
    observed: Rc<RefCell<Vec<bool>>>,
    source_slot: Rc<RefCell<Option<MutableInteractionSource>>>,
) {
    let source = rememberMutableInteractionSource();
    source_slot.borrow_mut().replace(source);
    let pressed = collect_is_pressed_as_state(&source);
    observed.borrow_mut().push(pressed.value());
}

#[test]
fn collect_is_pressed_as_state_recomposes_readers() {
    let observed = Rc::new(RefCell::new(Vec::<bool>::new()));
    let source_slot = Rc::new(RefCell::new(None::<MutableInteractionSource>));

    let mut composition = {
        let observed = Rc::clone(&observed);
        let source_slot = Rc::clone(&source_slot);
        crate::run_test_composition(move || {
            PressedReader(Rc::clone(&observed), Rc::clone(&source_slot));
        })
    };

    assert_eq!(observed.borrow().as_slice(), &[false]);

    let source = *source_slot
        .borrow()
        .as_ref()
        .expect("interaction source captured");
    let press = composition.with_app_context(|| source.press(Point { x: 1.0, y: 1.0 }));
    while composition
        .process_invalid_scopes()
        .expect("process press invalidation")
    {}
    assert_eq!(
        observed.borrow().last(),
        Some(&true),
        "press emission should recompose readers with pressed=true"
    );

    composition.with_app_context(|| source.release(press));
    while composition
        .process_invalid_scopes()
        .expect("process release invalidation")
    {}
    assert_eq!(
        observed.borrow().last(),
        Some(&false),
        "release emission should recompose readers with pressed=false"
    );
}

#[test]
fn interaction_source_exposes_latest_interaction() {
    let composition = Composition::new(MemoryApplier::new());
    let source = MutableInteractionSource::with_runtime(composition.runtime_handle());
    let last_interaction = source.collectLastInteractionAsState();

    assert_eq!(last_interaction.get(), None);

    let press = source.press(Point { x: 8.0, y: 12.0 });
    assert_eq!(
        last_interaction.get(),
        Some(Interaction::Press(PressInteraction::Press(press)))
    );
    assert_eq!(press.press_position, Point { x: 8.0, y: 12.0 });

    source.release(press);
    assert_eq!(
        last_interaction.get(),
        Some(Interaction::Press(PressInteraction::Release(
            PressInteractionRelease { press }
        )))
    );
}

#[test]
fn interaction_source_tracks_active_hover_state() {
    let composition = Composition::new(MemoryApplier::new());
    let source = MutableInteractionSource::with_runtime(composition.runtime_handle());
    let hovered = source.collectIsHoveredAsState();
    let pressed = source.collectIsPressedAsState();
    assert!(!hovered.get());

    let first = source.enter_hover();
    let second = source.enter_hover();
    assert!(hovered.get());
    assert!(!pressed.get(), "a hover is not a press");
    assert_ne!(first.id(), second.id());
    source.exit_hover(first);
    assert!(hovered.get(), "one hover still rests over the source");
    source.exit_hover(second);
    assert!(!hovered.get());
    assert_eq!(
        source.collectLastInteractionAsState().get(),
        Some(Interaction::Hover(HoverInteraction::Exit(
            HoverInteractionExit { enter: second }
        )))
    );
    assert!(!collect_is_hovered_as_state(&source).get());
}

fn hover_node(source: MutableInteractionSource, enabled: bool) -> InteractionNode<HoverTracker> {
    InteractionElement {
        interaction_source: source,
        tracker: HoverTracker { enabled },
    }
    .create()
}

fn deliver(node: &InteractionNode<HoverTracker>, kind: PointerEventKind) {
    let handler = node.pointer_input_handler().expect("a pointer handler");
    handler(PointerEvent::new(kind, Point::ZERO, Point::ZERO));
}

#[test]
fn a_hoverable_node_reports_the_pointer_entering_and_leaving() {
    let composition = Composition::new(MemoryApplier::new());
    let source = MutableInteractionSource::with_runtime(composition.runtime_handle());
    let hovered = source.collectIsHoveredAsState();
    let node = hover_node(source, true);

    deliver(&node, PointerEventKind::Enter);
    assert!(hovered.get());
    deliver(&node, PointerEventKind::Enter);
    deliver(&node, PointerEventKind::Exit);
    assert!(!hovered.get(), "a second enter does not stack");
    deliver(&node, PointerEventKind::Move);
    assert!(
        hovered.get(),
        "a move over a node that saw no enter, as one that appeared under the pointer, enters"
    );
}

#[test]
fn a_disabled_or_removed_hoverable_ends_its_hover() {
    let composition = Composition::new(MemoryApplier::new());
    let source = MutableInteractionSource::with_runtime(composition.runtime_handle());
    let hovered = source.collectIsHoveredAsState();

    let mut node = hover_node(source, false);
    deliver(&node, PointerEventKind::Enter);
    assert!(!hovered.get(), "a disabled node reports nothing");

    InteractionElement {
        interaction_source: source,
        tracker: HoverTracker { enabled: true },
    }
    .update(&mut node);
    deliver(&node, PointerEventKind::Enter);
    assert!(hovered.get());
    InteractionElement {
        interaction_source: source,
        tracker: HoverTracker { enabled: false },
    }
    .update(&mut node);
    assert!(!hovered.get(), "disabling the node ends its hover");

    let mut node = hover_node(source, true);
    deliver(&node, PointerEventKind::Enter);
    assert!(hovered.get());
    node.on_detach();
    assert!(!hovered.get(), "removing the node ends its hover");

    let other = MutableInteractionSource::with_runtime(composition.runtime_handle());
    let mut node = hover_node(source, true);
    deliver(&node, PointerEventKind::Enter);
    InteractionElement {
        interaction_source: other,
        tracker: HoverTracker { enabled: true },
    }
    .update(&mut node);
    assert!(
        !hovered.get(),
        "moving the node to another source ends the hover it reported to the first"
    );
}
