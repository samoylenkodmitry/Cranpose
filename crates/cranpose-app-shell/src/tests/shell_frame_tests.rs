use super::*;

#[test]
fn retained_redraw_traversal_keeps_wide_child_lists_and_clears_only_attached_nodes() {
    cranpose_ui::AppContext::new().enter(|| {
        let mut applier = MemoryApplier::new();
        let children: Vec<_> = (0..16)
            .map(|_| {
                let node = LayoutNode::new_virtual();
                node.clear_needs_redraw();
                applier.create(Box::new(node))
            })
            .collect();
        let detached = applier.create(Box::new(LayoutNode::new_virtual()));
        let mut parent = LayoutNode::new_virtual();
        parent.clear_needs_redraw();
        parent.children.clone_from(&children);
        let root = applier.create(Box::new(parent));
        for id in [children[0], children[15], detached] {
            applier
                .with_node::<LayoutNode, _>(id, |node| node.mark_needs_redraw())
                .expect("node to redraw");
        }

        let mut dirty = Vec::new();
        collect_retained_redraw_nodes(&mut applier, root, &mut dirty);
        assert_eq!(dirty, [children[0], children[15]]);
        dirty.clear();
        collect_retained_redraw_nodes(&mut applier, root, &mut dirty);
        assert!(dirty.is_empty());
        assert!(
            applier
                .with_node::<LayoutNode, _>(detached, |node| node.needs_redraw())
                .expect("detached node")
        );
    });
}

#[test]
fn the_frame_overlay_is_drawn_on_the_applications_own_window_only() {
    assert!(dev_overlay_belongs_on(RootId::Primary, true));
    assert!(
        !dev_overlay_belongs_on(RootId::Window(7), true),
        "a window of its own would repeat the application's numbers over content it does not own"
    );
    assert!(!dev_overlay_belongs_on(RootId::Primary, false));
}
