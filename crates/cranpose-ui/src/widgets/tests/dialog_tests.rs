use super::*;

#[test]
fn a_dialog_fills_the_window_inside_its_insets() {
    let viewport = Size {
        width: 400.0,
        height: 800.0,
    };
    let insets = cranpose_ui_graphics::EdgeInsets::from_components(8.0, 24.0, 8.0, 48.0);
    let bounds = dialog_bounds(viewport, insets);
    assert_eq!(bounds.x, 8.0);
    assert_eq!(bounds.y, 24.0);
    assert_eq!(bounds.width, 384.0);
    assert_eq!(bounds.height, 728.0);
}

#[test]
fn insets_larger_than_the_window_collapse_rather_than_going_negative() {
    let viewport = Size {
        width: 100.0,
        height: 100.0,
    };
    let insets = cranpose_ui_graphics::EdgeInsets::from_components(80.0, 80.0, 80.0, 80.0);
    let bounds = dialog_bounds(viewport, insets);
    assert_eq!(bounds.width, 0.0);
    assert_eq!(bounds.height, 0.0);
}

#[test]
fn a_required_dialog_refuses_both_dismissals_but_still_covers_the_screen() {
    let spec = DialogSpec::required();
    assert!(!spec.dismiss_on_outside_tap);
    assert!(!spec.dismiss_on_back);
    assert!(spec.apply_window_insets);
}

#[test]
fn the_default_dialog_dismisses_both_ways_until_told_otherwise() {
    let spec = DialogSpec::default();
    assert!(spec.dismiss_on_outside_tap);
    assert!(spec.dismiss_on_back);
    assert!(!spec.with_dismiss_on_back(false).dismiss_on_back);
    assert!(
        !spec
            .with_dismiss_on_outside_tap(false)
            .dismiss_on_outside_tap
    );
    assert!(!spec.with_window_insets(false).apply_window_insets);
}

#[test]
fn a_dialog_traps_focus_inside_it_and_releases_it_on_close() {
    use std::cell::{Cell, RefCell};

    use cranpose_core::{
        Composition, MemoryApplier, MutableState, NodeId, location_key, mutableStateOf, remember,
    };
    use cranpose_foundation::{PointerEvent, PointerEventKind, text::TextFieldState};
    use cranpose_ui_graphics::Size;

    use crate::{
        layout::{LayoutBox, LayoutEngine, LayoutTree},
        text::TextStyle,
        text_field_focus::{focused_editor_state, has_focused_field},
        widgets::{basic_text_field::BasicTextField, popup::PopupHost},
    };

    let _app_context = crate::render_state::app_context_test_scope();
    crate::modal::clear_modals();

    let mut composition = Composition::new(MemoryApplier::new());

    let outside_state = TextFieldState::new("outside");
    let inside_state = TextFieldState::new("inside");
    let outside_id: Rc<Cell<Option<NodeId>>> = Rc::new(Cell::new(None));
    let inside_id: Rc<Cell<Option<NodeId>>> = Rc::new(Cell::new(None));
    let dialog_open_slot: Rc<RefCell<Option<MutableState<bool>>>> = Rc::new(RefCell::new(None));

    let mut content = {
        let outside_id = Rc::clone(&outside_id);
        let inside_id = Rc::clone(&inside_id);
        let dialog_open_slot = Rc::clone(&dialog_open_slot);
        move || {
            let outside_id = Rc::clone(&outside_id);
            let inside_id = Rc::clone(&inside_id);
            let dialog_open_slot = Rc::clone(&dialog_open_slot);
            PopupHost(move || {
                let dialog_open = remember(|| mutableStateOf(true)).with(|state| *state);
                *dialog_open_slot.borrow_mut() = Some(dialog_open);

                outside_id.set(Some(BasicTextField(
                    outside_state,
                    Modifier::empty(),
                    TextStyle::default(),
                )));

                if dialog_open.value() {
                    let inside_id = Rc::clone(&inside_id);
                    Dialog(
                        DialogSpec::default(),
                        |_reason: DismissReason| {},
                        move || {
                            inside_id.set(Some(BasicTextField(
                                inside_state,
                                Modifier::empty(),
                                TextStyle::default(),
                            )));
                        },
                    );
                }
            });
        }
    };

    let key = location_key(file!(), line!(), column!());
    composition.render(key, &mut content).expect("render");

    let mut settle = move |composition: &mut Composition<MemoryApplier>| -> LayoutTree {
        for _ in 0..16 {
            if !composition.should_render() {
                break;
            }
            composition.reconcile(key, &mut content).expect("reconcile");
        }
        let root = composition.root().expect("root");
        let handle = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let layout = applier
            .compute_layout(root, Size::new(400.0, 800.0))
            .expect("layout");
        applier.clear_runtime_handle();
        drop(applier);
        layout
    };

    fn find_node(node: &LayoutBox, id: NodeId) -> Option<&LayoutBox> {
        if node.node_id == id {
            return Some(node);
        }
        node.children.iter().find_map(|child| find_node(child, id))
    }

    fn tap(layout_root: &LayoutBox, id: &Rc<Cell<Option<NodeId>>>) {
        let id = id.get().expect("field composed");
        let node = find_node(layout_root, id).expect("field placed in the layout");
        let handler = node
            .node_data
            .modifier_slices()
            .pointer_inputs()
            .first()
            .cloned()
            .expect("field has a pointer handler");
        let position = cranpose_ui_graphics::Point { x: 1.0, y: 1.0 };
        handler(PointerEvent::new(
            PointerEventKind::Down,
            position,
            position,
        ));
    }

    let mut layout = settle(&mut composition);
    for _ in 0..7 {
        layout = settle(&mut composition);
    }

    tap(layout.root(), &inside_id);
    assert!(has_focused_field(), "the dialog's own field must focus");
    assert_eq!(
        focused_editor_state().map(|s| s.text),
        Some("inside".to_string()),
        "the dialog's own field must be the one focused"
    );

    tap(layout.root(), &outside_id);
    assert_eq!(
        focused_editor_state().map(|s| s.text),
        Some("inside".to_string()),
        "a field behind an open dialog must not take focus from it"
    );

    let dialog_open = dialog_open_slot
        .borrow()
        .as_ref()
        .copied()
        .expect("dialog_open captured");
    dialog_open.set(false);
    layout = settle(&mut composition);
    assert!(
        !has_focused_field(),
        "closing the dialog must release focus from the field it held"
    );

    tap(layout.root(), &outside_id);
    assert_eq!(
        focused_editor_state().map(|s| s.text),
        Some("outside".to_string()),
        "once the dialog is closed, the field behind it must be focusable"
    );

    crate::modal::clear_modals();
    crate::text_field_focus::clear_focus();
}
