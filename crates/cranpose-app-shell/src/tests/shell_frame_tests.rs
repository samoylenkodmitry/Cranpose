use super::*;

#[test]
fn a_redraw_asked_before_the_id_reaches_the_surface_once_and_waits_while_detached() {
    let context = cranpose_ui::AppContext::new();
    context.enter(|| {
        let mut applier = MemoryApplier::new();
        let parent = LayoutNode::new_virtual();
        parent.clear_needs_redraw();
        let root = applier.create(Box::new(parent));
        // A new node asks for a redraw before it has an id.
        let children: Vec<_> = (0..16)
            .map(|index| {
                let node = LayoutNode::new_virtual();
                if index != 0 && index != 15 {
                    node.clear_needs_redraw();
                }
                applier.create(Box::new(node))
            })
            .collect();
        let detached = applier.create(Box::new(LayoutNode::new_virtual()));
        applier
            .with_node::<LayoutNode, _>(root, |node| node.children.clone_from(&children))
            .expect("root");
        for &id in [root].iter().chain(&children).chain([&detached]) {
            applier
                .with_node::<LayoutNode, _>(id, |node| node.set_node_id(id))
                .expect("node");
        }
        let attach = |applier: &mut MemoryApplier, child| {
            applier
                .with_node::<LayoutNode, _>(child, |node| {
                    cranpose_core::Node::on_attached_to_parent(node, root);
                })
                .expect("child");
        };
        for &child in &children {
            attach(&mut applier, child);
        }

        let mut dirty = Vec::new();
        let mut pending = Vec::new();
        collect_retained_redraw_nodes(&mut applier, &context, root, &mut dirty, &mut pending);
        assert_eq!(dirty, [children[0], children[15]]);
        dirty.clear();
        collect_retained_redraw_nodes(&mut applier, &context, root, &mut dirty, &mut pending);
        assert!(dirty.is_empty(), "a frame takes each redraw once");
        assert!(
            applier
                .with_node::<LayoutNode, _>(detached, |node| node.needs_redraw())
                .expect("detached node"),
            "a node not attached keeps its redraw"
        );

        attach(&mut applier, detached);
        collect_retained_redraw_nodes(&mut applier, &context, root, &mut dirty, &mut pending);
        assert_eq!(dirty, [detached], "the frame after its attach takes it");
    });
}

#[test]
fn the_frame_overlay_is_drawn_on_the_applications_own_window_only() {
    assert!(dev_overlay_belongs_on(RootId::Primary, true));
    assert!(
        !dev_overlay_belongs_on(RootId::Window(NodeId(7)), true),
        "a window of its own would repeat the application's numbers over content it does not own"
    );
    assert!(!dev_overlay_belongs_on(RootId::Primary, false));
}
