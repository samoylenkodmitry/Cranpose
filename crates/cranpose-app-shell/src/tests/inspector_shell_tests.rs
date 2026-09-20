use super::*;
use crate::inspector::{InspectorAction, InspectorNode};

fn project(layout: &LayoutTree, _semantics: &cranpose_ui::SemanticsTree) -> Vec<InspectorNode> {
    vec![InspectorNode {
        node_id: layout.root().node_id,
        canvas_key: None,
        bounds: layout.root().rect,
        label: "App".into(),
        details: "Application element".into(),
        focused: false,
        issue: false,
    }]
}

fn press(shell: &mut AppShell<TestRenderer>, action: InspectorAction) {
    let bounds = shell
        .inspector_state()
        .controls
        .iter()
        .find(|control| control.action == action)
        .expect("inspector control")
        .bounds;
    shell.set_cursor(
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    );
    assert!(shell.pointer_pressed());
    assert!(shell.pointer_released());
    shell.update();
}

#[test]
fn reader_activation_bypasses_picking_and_cancel_releases_inspector_capture() {
    let _guard = test_guard();
    let mut shell = AppShell::new(
        HitGraphRenderer::default(),
        location_key(file!(), line!(), column!()),
        app_shell_demo_like_clickable_tab_host,
    );
    shell.set_inspector_projector(Some(project));
    shell.update();
    let bounds =
        find_layout_box_with_text(shell.layout_tree().expect("layout").root(), "Increment")
            .expect("button")
            .rect;
    let (x, y) = (
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    );
    shell.on_key_event(&KeyEvent::key_down_with_modifiers(
        KeyCode::I,
        "",
        Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        },
    ));
    shell.on_key_event(&KeyEvent::key_down(KeyCode::P, "p"));
    shell.update();
    shell.set_cursor(x, y);
    shell.pointer_pressed();
    shell.pointer_released();
    shell.update();
    assert!(
        layout_tree_texts(shell.layout_tree().expect("picked layout"))
            .iter()
            .any(|text| text == "Counter: 0")
    );
    shell.on_key_event(&KeyEvent::key_down(KeyCode::P, "p"));
    assert!(shell.accessibility_activate_at(x, y));
    shell.update();
    assert!(shell.inspector_state().picking);
    assert!(
        layout_tree_texts(shell.layout_tree().expect("activated layout"))
            .iter()
            .any(|text| text == "Counter: 1")
    );
    assert!(shell.primary().accessibility_activate_at(x, y));
    shell.update();
    assert!(
        layout_tree_texts(shell.layout_tree().expect("second activation"))
            .iter()
            .any(|text| text == "Counter: 2")
    );
    shell.surfaces[0].inspector.pointer_captured = true;
    shell.cancel_gesture_unless_pressed();
    assert!(shell.surfaces[0].inspector.pointer_captured);
    shell.cancel_gesture();
    assert!(!shell.surfaces[0].inspector.pointer_captured);
}

#[test]
fn inspector_collects_only_while_open_and_never_enters_the_app_tree() {
    let _guard = test_guard();
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        box_content,
    );
    shell.set_semantics_enabled(true);
    shell.update();
    let before = shell
        .semantics_tree()
        .expect("app semantics")
        .root()
        .node_id;
    shell.set_inspector_projector(Some(project));
    shell.update();
    assert!(!shell.inspector_state().open);
    assert!(shell.inspector_state().nodes.is_empty());
    press(&mut shell, InspectorAction::Toggle);
    assert_eq!(shell.inspector_state().nodes.len(), 1);
    assert_eq!(
        shell
            .semantics_tree()
            .expect("app semantics")
            .root()
            .node_id,
        before
    );
    let state = shell.inspector_state().clone();
    assert_eq!(shell.primary().inspector_state(), &state);
    assert!(
        !shell.update().visual_changed,
        "stable inspector does not force frames"
    );
    press(&mut shell, InspectorAction::Toggle);
    assert!(shell.inspector_state().nodes.is_empty());
    shell.set_inspector_projector(None);
    shell.update();
    assert!(shell.inspector_state().controls.is_empty());
    assert!(!shell.primary().inspector_press(790.0, 590.0));
}

#[test]
fn inspector_can_read_semantics_without_enabling_a_platform_bridge() {
    let _guard = test_guard();
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        box_content,
    );
    assert!(!shell.semantics_active());
    shell.set_inspector_projector(Some(project));
    shell.update();
    press(&mut shell, InspectorAction::Toggle);
    assert_eq!(shell.inspector_state().nodes.len(), 1);
    assert!(!shell.semantics_active());
}

#[test]
fn shortcut_and_navigation_preserve_app_focus() {
    let _guard = test_guard();
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        box_content,
    );
    shell.set_inspector_projector(Some(project));
    shell.update();
    let event = KeyEvent::key_down_with_modifiers(
        KeyCode::I,
        "",
        Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        },
    );
    let focus = shell.debug_enter_app_context(cranpose_ui::active_focus_target);
    assert!(shell.on_key_event(&event));
    shell.update();
    assert!(shell.inspector_state().open);
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::A, "a")));
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Tab, "\t")));
    assert!(shell.on_paste("not for the app"));
    assert!(shell.on_ime_preedit("not for the app", None));
    assert!(shell.on_ime_set_selection(0, 1));
    assert!(shell.on_ime_set_composing_region(0, 1));
    assert!(shell.on_ime_finish_composing());
    assert!(shell.on_ime_delete_surrounding(1, 1));
    assert!(shell.on_copy().is_none());
    assert!(shell.on_cut().is_none());
    assert_eq!(
        shell.debug_enter_app_context(cranpose_ui::active_focus_target),
        focus
    );
    assert!(shell.on_key_event(&KeyEvent::key_down(KeyCode::Escape, "")));
    shell.update();
    assert!(!shell.inspector_state().open);
}
