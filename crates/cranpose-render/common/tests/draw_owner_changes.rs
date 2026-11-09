use cranpose_core::NodeId;
use cranpose_render_common::DrawOwnerChange::{self, Joined, Left};

#[test]
fn a_node_one_scene_left_and_another_joined_stays_drawn() {
    // Two scenes' changes in one frame: node 1 moved from the first scene
    // to the second, node 3 left the second, node 2 joined the first, and
    // node 4 left both.
    let mut changes = vec![
        Left(NodeId(4)),
        Left(NodeId(1)),
        Joined(NodeId(2)),
        Left(NodeId(3)),
        Joined(NodeId(1)),
        Left(NodeId(4)),
    ];
    DrawOwnerChange::retain_undrawn(&mut changes);
    assert_eq!(changes, [Left(NodeId(3)), Left(NodeId(4))]);
}
