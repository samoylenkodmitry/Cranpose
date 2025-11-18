use super::*;
use crate::layout::{LayoutBox, LayoutNodeData, LayoutNodeKind};
use crate::modifier::{Modifier, ModifierNodeSlices, Rect, ResolvedModifiers};

#[test]
fn test_count_nodes() {
    let empty_rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    let node_data = || {
        LayoutNodeData::new(
            Modifier::empty(),
            ResolvedModifiers::default(),
            ModifierNodeSlices::default(),
            LayoutNodeKind::Unknown,
        )
    };
    let root = LayoutBox {
        node_id: 0,
        rect: empty_rect,
        node_data: node_data(),
        children: vec![
            LayoutBox {
                node_id: 1,
                rect: empty_rect,
                node_data: node_data(),
                children: vec![],
            },
            LayoutBox {
                node_id: 2,
                rect: empty_rect,
                node_data: node_data(),
                children: vec![],
            },
        ],
    };

    assert_eq!(count_nodes(&root), 3);
}
