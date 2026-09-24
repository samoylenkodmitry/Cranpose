use std::rc::Rc;

use super::*;
use crate::graph::HitTestNode;

type RecordedHit = (
    NodeId,
    Vec<NodeId>,
    Rect,
    [[f32; 2]; 4],
    Option<Rect>,
    usize,
);

#[derive(Default)]
struct TestSink {
    hits: Vec<RecordedHit>,
}

impl HitGraphSink for TestSink {
    fn push_hit(
        &mut self,
        node_id: NodeId,
        capture_path: &[NodeId],
        geometry: HitGeometry<'_>,
        _hit: &HitTestNode,
    ) {
        self.hits.push((
            node_id,
            capture_path.to_vec(),
            geometry.rect,
            geometry.quad,
            geometry.hit_clip_bounds,
            geometry.hit_clips.len(),
        ));
    }
}

fn test_layer(node_id: NodeId, transform_to_parent: ProjectiveTransform) -> LayerNode {
    LayerNode {
        node_id: Some(node_id),
        local_bounds: Rect {
            x: 0.0,
            y: 0.0,
            width: 30.0,
            height: 18.0,
        },
        transform_to_parent,
        clip_to_bounds: true,
        hit_test: Some(HitTestNode {
            shape: None,
            click_actions: vec![Rc::new(|_point| {})],
            pointer_inputs: vec![],
            pointer_icon: None,
            clip: None,
        }),
        has_hit_targets: true,
        ..Default::default()
    }
}

#[test]
fn collect_hits_uses_graph_transform_to_parent() {
    let layer = test_layer(7, ProjectiveTransform::translation(12.0, 9.0));
    let mut sink = TestSink::default();

    collect_hits_from_graph(&layer, ProjectiveTransform::identity(), &mut sink, None);

    assert_eq!(sink.hits.len(), 1);
    let (node_id, capture_path, rect, quad, clip, clip_count) = &sink.hits[0];
    assert_eq!(*node_id, 7);
    assert_eq!(capture_path, &vec![7]);
    assert_eq!(
        *rect,
        Rect {
            x: 12.0,
            y: 9.0,
            width: 30.0,
            height: 18.0,
        }
    );
    assert_eq!(
        *quad,
        [[12.0, 9.0], [42.0, 9.0], [12.0, 27.0], [42.0, 27.0]]
    );
    assert_eq!(*clip, Some(*rect));
    assert_eq!(*clip_count, 1);
}

#[test]
fn a_child_clipped_away_by_its_parent_takes_no_hits() {
    let mut list = test_layer(1, ProjectiveTransform::translation(0.0, 80.0));
    let scrolled_out = test_layer(2, ProjectiveTransform::translation(4.0, -40.0));
    list.children
        .push(RenderNode::Layer(Box::new(scrolled_out)));
    let mut sink = TestSink::default();

    collect_hits_from_graph(&list, ProjectiveTransform::identity(), &mut sink, None);

    let hit_ids: Vec<NodeId> = sink.hits.iter().map(|hit| hit.0).collect();
    assert_eq!(
        hit_ids,
        vec![1],
        "a child the list has scrolled past its edge lies outside the list's clip and \
         takes no hits, however far inside the window it sits"
    );
}

#[test]
fn collect_hits_composes_nested_graph_transforms() {
    let child = test_layer(9, ProjectiveTransform::translation(4.0, 3.0));
    let mut parent = test_layer(7, ProjectiveTransform::translation(10.0, 6.0));
    parent.hit_test.as_mut().expect("hit test").pointer_inputs = vec![Rc::new(|_event| {})];
    parent.children.push(RenderNode::Layer(Box::new(child)));
    let mut sink = TestSink::default();

    collect_hits_from_graph(&parent, ProjectiveTransform::identity(), &mut sink, None);

    assert_eq!(sink.hits.len(), 2);
    let (_, child_capture_path, child_rect, child_quad, child_clip, child_clip_count) =
        &sink.hits[1];
    assert_eq!(child_capture_path, &vec![9, 7]);
    assert_eq!(
        *child_rect,
        Rect {
            x: 14.0,
            y: 9.0,
            width: 30.0,
            height: 18.0,
        }
    );
    assert_eq!(
        *child_quad,
        [[14.0, 9.0], [44.0, 9.0], [14.0, 27.0], [44.0, 27.0]]
    );
    assert_eq!(
        *child_clip,
        Some(Rect {
            x: 14.0,
            y: 9.0,
            width: 26.0,
            height: 15.0,
        })
    );
    assert_eq!(*child_clip_count, 2);
}

#[test]
fn capture_paths_do_not_leak_between_siblings_or_traversals() {
    let identity = ProjectiveTransform::identity();
    let mut root = test_layer(12, identity);
    for node_id in (1..12).rev() {
        let mut parent = test_layer(node_id, identity);
        parent.hit_test.as_mut().unwrap().pointer_inputs = vec![Rc::new(|_| {})];
        parent.children.push(RenderNode::Layer(Box::new(root)));
        root = parent;
    }
    root.children
        .push(RenderNode::Layer(Box::new(test_layer(13, identity))));
    let mut sink = TestSink::default();
    collect_hits_from_graph(&root, identity, &mut sink, None);
    collect_hits_from_graph(&test_layer(14, identity), identity, &mut sink, None);
    let mut expected: Vec<Vec<_>> = (1..=12)
        .map(|node_id| (1..=node_id).rev().collect())
        .collect();
    expected.extend([vec![13, 1], vec![14]]);
    let paths: Vec<_> = sink.hits.into_iter().map(|hit| hit.1).collect();
    assert_eq!(paths, expected);
}

#[test]
fn collect_hits_retains_transformed_clip_chain() {
    let mut parent = test_layer(1, ProjectiveTransform::translation(20.0, 10.0));
    let mut child = test_layer(
        2,
        ProjectiveTransform::from_rect_to_quad(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 30.0,
                height: 18.0,
            },
            [[0.0, 0.0], [30.0, 0.0], [4.0, 18.0], [34.0, 18.0]],
        ),
    );
    child.clip_to_bounds = true;
    parent.children.push(RenderNode::Layer(Box::new(child)));

    let mut sink = TestSink::default();
    collect_hits_from_graph(&parent, ProjectiveTransform::identity(), &mut sink, None);

    let (_, _, _, _, _, child_clip_count) = sink.hits[1];
    assert_eq!(child_clip_count, 2);
}
