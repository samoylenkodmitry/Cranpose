use std::rc::Rc;

use cranpose_ui::{LayoutNodeData, LayoutNodeKind, Modifier, Point, Rect};

use super::*;

fn layout(id: usize, children: Vec<LayoutBox>) -> LayoutBox {
    LayoutBox::new(
        id,
        Rect {
            x: 12.0,
            y: 18.0,
            width: 100.0,
            height: 40.0,
        },
        Point::default(),
        LayoutNodeData::new(
            Modifier::empty().padding(8.0),
            Default::default(),
            Rc::new(Default::default()),
            None,
            LayoutNodeKind::Layout,
        ),
        children,
    )
}

#[test]
fn empty_snapshot_keeps_request_identity() {
    let value = snapshot(None, 41, 20);
    assert_eq!(value.request_id, 41);
    assert_eq!(value.schema, 2);
    assert!(!value.truncated);
    assert!(value.nodes.is_empty());
}

#[test]
fn snapshot_preserves_hierarchy_geometry_and_generation() {
    let mut child = layout(2, Vec::new());
    child.node_generation = 7;
    let tree = LayoutTree::new(layout(1, vec![child]));
    let value = snapshot(Some(&tree), 8, 20);
    assert_eq!(value.nodes.len(), 2);
    assert_eq!(value.nodes[1].id, "2:7");
    assert_eq!(value.nodes[1].parent.as_deref(), Some("1:0"));
    assert_eq!(
        (
            value.nodes[1].x,
            value.nodes[1].y,
            value.nodes[1].width,
            value.nodes[1].height
        ),
        (12.0, 18.0, 100.0, 40.0)
    );
    let json = serde_json::to_value(value).expect("snapshot encodes");
    assert_eq!(json["requestId"], 8);
}

#[test]
fn node_budget_never_leaves_orphaned_children() {
    let tree = LayoutTree::new(layout(1, vec![layout(2, vec![layout(3, Vec::new())])]));
    let value = snapshot(Some(&tree), 1, 2);
    assert_eq!(value.nodes.len(), 2);
    assert!(value.truncated);
    assert_eq!(
        value.nodes[1].parent.as_deref(),
        Some(value.nodes[0].id.as_str())
    );
    assert!(snapshot(Some(&tree), 2, 0).nodes.is_empty());
}
