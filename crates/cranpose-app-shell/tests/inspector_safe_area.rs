//! On a phone the system draws over the window's edges: the status bar, the
//! rounded corners and the home indicator. The developer inspector keeps its
//! launcher, its panel and the panel's close button inside the safe area, also
//! after the panel is dragged against an edge, so a finger can always close it.

use cranpose_app_shell::{
    AppShell,
    inspector::{InspectorAction, InspectorNode},
};
use cranpose_render_common::Renderer;
use cranpose_ui::{Modifier, Text, TextStyle};
use cranpose_ui_graphics::{EdgeInsets, Rect};

mod support;
use support::checking_shell;

const WIDTH: f32 = 440.0;
const HEIGHT: f32 = 956.0;
const INSETS: EdgeInsets = EdgeInsets {
    left: 0.0,
    top: 62.0,
    right: 0.0,
    bottom: 34.0,
};

fn project(semantics: &cranpose_ui::SemanticsTree) -> Vec<InspectorNode> {
    vec![InspectorNode {
        node_id: semantics.root().node_id,
        canvas_key: None,
        bounds: semantics.root().bounds,
        label: "App".into(),
        details: "Application element".into(),
        focused: false,
        issue: false,
    }]
}

fn control<R: Renderer>(shell: &AppShell<R>, action: InspectorAction) -> Rect
where
    R::Error: std::fmt::Debug,
{
    shell
        .inspector_state()
        .controls
        .iter()
        .find(|control| control.action == action)
        .unwrap_or_else(|| panic!("no {action:?} control"))
        .bounds
}

fn assert_controls_are_safe<R: Renderer>(shell: &AppShell<R>)
where
    R::Error: std::fmt::Debug,
{
    for control in &shell.inspector_state().controls {
        let bounds = control.bounds;
        assert!(
            bounds.x >= INSETS.left
                && bounds.y >= INSETS.top
                && bounds.x + bounds.width <= WIDTH - INSETS.right
                && bounds.y + bounds.height <= HEIGHT - INSETS.bottom,
            "{:?} at {bounds:?} reaches into the system's edges",
            control.action
        );
    }
}

#[test]
fn the_inspector_stays_inside_the_safe_area_where_it_can_be_closed() {
    let (mut shell, _) = checking_shell(false, || {
        Text("Screen", Modifier::empty(), TextStyle::default());
    });
    shell.set_viewport(WIDTH, HEIGHT);
    // The insets may come before the inspector is installed.
    shell.set_safe_area(INSETS);
    shell.set_inspector_projector(Some(project));
    shell.update();
    assert_controls_are_safe(&shell);

    let launcher = control(&shell, InspectorAction::Toggle);
    shell.set_cursor(
        launcher.x + launcher.width / 2.0,
        launcher.y + launcher.height / 2.0,
    );
    shell.pointer_pressed();
    shell.pointer_released();
    shell.update();
    assert!(
        shell.inspector_state().open,
        "the launcher did not open the panel"
    );
    assert_controls_are_safe(&shell);

    // Dragged by its title bar against the top edge, the panel stops at the
    // safe area.
    let title = control(&shell, InspectorAction::Move);
    shell.set_cursor(title.x + 20.0, title.y + title.height / 2.0);
    shell.pointer_pressed();
    shell.set_cursor(title.x + 20.0, title.y - 200.0);
    shell.pointer_released();
    shell.update();
    assert_controls_are_safe(&shell);

    let close = control(&shell, InspectorAction::Toggle);
    shell.set_cursor(close.x + close.width / 2.0, close.y + close.height / 2.0);
    shell.pointer_pressed();
    shell.pointer_released();
    shell.update();
    assert!(
        !shell.inspector_state().open,
        "the close button did not close the panel"
    );
}
