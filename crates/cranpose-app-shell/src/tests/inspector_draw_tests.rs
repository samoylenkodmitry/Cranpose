use super::*;

// The inspector's graph at `viewport`, its text laid out in an app context
// as a frame lays it out.
fn built(state: &mut InspectorState, viewport: Size) -> RenderGraph {
    cranpose_ui::AppContext::new().enter(|| build(state, viewport, Rect::from_size(viewport)))
}

#[test]
fn collapsed_control_and_panel_fit_phone_and_desktop_viewports() {
    for viewport in [
        Size {
            width: 192.0,
            height: 320.0,
        },
        Size {
            width: 900.0,
            height: 700.0,
        },
    ] {
        let mut state = InspectorState::default();
        built(&mut state, viewport);
        assert_eq!(state.controls.len(), 1);
        assert_eq!(state.controls[0].action, InspectorAction::Toggle);
        state.open = true;
        built(&mut state, viewport);
        for control in state.controls {
            assert!(control.bounds.x >= 0.0 && control.bounds.y >= 0.0);
            assert!(control.bounds.x + control.bounds.width <= viewport.width);
            assert!(control.bounds.y + control.bounds.height <= viewport.height);
        }
    }
}

#[test]
fn every_mode_has_a_distinct_graph_and_only_mode_has_opaque_background() {
    let viewport = Size {
        width: 900.0,
        height: 700.0,
    };
    let mut state = InspectorState {
        open: true,
        mode: InspectorMode::Accessibility,
        ..Default::default()
    };
    let graph = built(&mut state, viewport);
    let RenderNode::Primitive(entry) = &graph.root.children[0] else {
        panic!("opaque background")
    };
    let PrimitiveNode::Draw(draw) = &entry.node else {
        panic!("background primitive")
    };
    let DrawPrimitive::RoundRect {
        rect,
        brush: Brush::Solid(color),
        ..
    } = draw.primitive
    else {
        panic!("solid background")
    };
    assert_eq!(rect, Rect::from_size(viewport));
    assert_eq!(color.3, 1.0);
}

#[test]
fn unicode_labels_are_truncated_and_wrapped_at_character_boundaries() {
    assert_eq!(truncate("A😀éB", 21.0), "A😀…");
    assert_eq!(wrapped_lines("A😀éB\n\nZ", 14.0), ["A😀", "éB", "", "Z"]);
}

#[test]
fn picking_hides_panel_to_allow_inspecting_covered_elements() {
    let mut state = InspectorState {
        open: true,
        picking: true,
        ..Default::default()
    };
    let viewport = Size {
        width: 400.0,
        height: 400.0,
    };
    built(&mut state, viewport);
    assert_eq!(state.controls.len(), 1);
    assert_eq!(state.controls[0].action, InspectorAction::Pick);
}

#[test]
fn floating_positions_remain_reachable_after_resize() {
    let viewport = Size {
        width: 192.0,
        height: 320.0,
    };
    let mut state = InspectorState {
        launcher_position: Some(Point {
            x: 1000.0,
            y: -100.0,
        }),
        panel_position: Some(Point { x: -50.0, y: 900.0 }),
        ..Default::default()
    };
    built(&mut state, viewport);
    assert_eq!(state.launcher_position, Some(Point { x: 46.0, y: 0.0 }));
    state.open = true;
    built(&mut state, viewport);
    assert_eq!(state.panel_position, Some(Point { x: 0.0, y: 16.0 }));
    for control in state.controls {
        assert!(control.bounds.x >= 0.0 && control.bounds.y >= 0.0);
        assert!(control.bounds.x + control.bounds.width <= viewport.width);
        assert!(control.bounds.y + control.bounds.height <= viewport.height);
    }
}
