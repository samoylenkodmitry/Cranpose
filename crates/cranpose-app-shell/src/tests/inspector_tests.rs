use super::*;

fn node(id: usize, key: Option<u64>, bounds: Rect) -> InspectorNode {
    InspectorNode {
        node_id: id,
        canvas_key: key,
        bounds,
        label: format!("Node {id}"),
        details: "Name: sample\nRole: Button".into(),
        focused: false,
        issue: false,
    }
}

#[test]
fn selection_follows_identity_and_disappears_with_the_node() {
    let a = node(10, Some(1), Rect::from_size(Size::default()));
    let b = node(10, Some(2), Rect::from_size(Size::default()));
    let mut inspector = DeveloperInspector::default();
    inspector.replace_nodes(vec![a.clone(), b.clone()]);
    inspector.apply(InspectorAction::Select(1));
    inspector.replace_nodes(vec![b.clone(), a]);
    assert_eq!(inspector.state.selected, Some(0));
    inspector.replace_nodes(Vec::new());
    assert_eq!(inspector.state.selected, None);
    inspector.apply(InspectorAction::Next);
    assert_eq!(inspector.state.selected, None);
}

#[test]
fn picker_selects_smallest_containing_control_without_an_action() {
    let mut inspector = DeveloperInspector::default();
    inspector.state.nodes = vec![
        node(
            1,
            None,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 200.0,
            },
        ),
        node(
            2,
            None,
            Rect {
                x: 10.0,
                y: 10.0,
                width: 40.0,
                height: 30.0,
            },
        ),
    ];
    inspector.apply(InspectorAction::Pick);
    inspector.pick(20.0, 20.0);
    assert_eq!(inspector.state.selected, Some(1));
    assert!(!inspector.state.picking);
    inspector.pick(500.0, 500.0);
    assert_eq!(inspector.state.selected, None);
}

#[test]
fn controls_cycle_selection_and_closing_restores_normal_view() {
    let mut inspector = DeveloperInspector::default();
    inspector.apply(InspectorAction::Toggle);
    inspector.state.nodes = vec![
        node(1, None, Rect::from_size(Size::default())),
        node(2, None, Rect::from_size(Size::default())),
    ];
    inspector.apply(InspectorAction::Previous);
    assert_eq!(inspector.state.selected, Some(1));
    inspector.apply(InspectorAction::Next);
    assert_eq!(inspector.state.selected, Some(0));
    for (action, mode) in [
        (InspectorAction::Overlay, InspectorMode::Overlay),
        (InspectorAction::Accessibility, InspectorMode::Accessibility),
        (InspectorAction::Normal, InspectorMode::Normal),
    ] {
        inspector.apply(action);
        assert_eq!(inspector.state.mode, mode);
    }
    inspector.apply(InspectorAction::Toggle);
    assert!(!inspector.state.open);
    assert_eq!(inspector.state.mode, InspectorMode::Normal);
    assert!(inspector.state.nodes.is_empty());
}

#[test]
fn detail_scrolling_is_bounded_and_selection_resets_it() {
    let mut inspector = DeveloperInspector {
        viewport: Some(Size {
            width: 400.0,
            height: 400.0,
        }),
        ..Default::default()
    };
    inspector.state.nodes = vec![node(1, None, Rect::from_size(Size::default()))];
    inspector.apply(InspectorAction::Select(0));
    for _ in 0..10 {
        inspector.apply(InspectorAction::DetailsDown);
    }
    assert_eq!(inspector.state.detail_offset, 1);
    inspector.apply(InspectorAction::DetailsUp);
    assert_eq!(inspector.state.detail_offset, 0);
}
