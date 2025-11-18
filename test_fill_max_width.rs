// Test case to reproduce fill_max_width issue

#[cfg(test)]
mod test {
    use compose_ui::*;
    use compose_core::*;
    use compose_testing::*;

    #[test]
    fn test_fill_max_width_with_padding() {
        use std::rc::Rc;
        use std::cell::RefCell;

        let mut composition = Composition::new(MemoryApplier::new());
        let key = location_key(file!(), line!(), column!());

        let row_id: Rc<RefCell<Option<NodeId>>> = Rc::new(RefCell::new(None));
        let row_id_render = Rc::clone(&row_id);

        composition
            .render(key, move || {
                let row_capture = Rc::clone(&row_id_render);

                // Outer Column with padding(20.0)
                Column(Modifier::empty().padding(20.0), ColumnSpec::default(), move || {
                    // Row with fill_max_width() and padding(8.0)
                    *row_capture.borrow_mut() = Some(Row(
                        Modifier::empty().fill_max_width().then(Modifier::empty().padding(8.0)),
                        RowSpec::default(),
                        move || {
                            Text("Button 1", Modifier::empty().padding(4.0));
                            Text("Button 2", Modifier::empty().padding(4.0));
                        },
                    ));
                });
            })
            .expect("initial render");

        let root = composition.root().expect("root node");
        let layout_tree = composition
            .applier_mut()
            .compute_layout(
                root,
                Size {
                    width: 800.0,
                    height: 600.0,
                },
            )
            .expect("compute layout");

        let root_layout = layout_tree.root();

        // Find the Row layout
        fn find_layout<'a>(node: &'a LayoutBox, target: NodeId) -> Option<&'a LayoutBox> {
            if node.node_id == target {
                return Some(node);
            }
            node.children
                .iter()
                .find_map(|child| find_layout(child, target))
        }

        let row_node_id = row_id.borrow().as_ref().copied().expect("row node id");
        let row_layout = find_layout(&root_layout, row_node_id).expect("row layout");

        // Expected:
        // Window: 800px
        // Column padding: 40px (20 on each side)
        // Column inner width: 760px
        // Row should be: 760px max

        println!("Root width: {}", root_layout.rect.width);
        println!("Row x: {}, width: {}", row_layout.rect.x, row_layout.rect.width);
        println!("Row right edge: {}", row_layout.rect.x + row_layout.rect.width);
        println!("Root right edge: {}", root_layout.rect.x + root_layout.rect.width);

        // Row should not exceed the root width
        assert!(
            row_layout.rect.x + row_layout.rect.width <= root_layout.rect.width + 0.001,
            "Row overflows root: Row right edge={} > Root width={}",
            row_layout.rect.x + row_layout.rect.width,
            root_layout.rect.width
        );

        // Row should be within the Column's inner area
        // Column inner width = 800 - 40 = 760
        assert!(
            row_layout.rect.width <= 760.0 + 0.001,
            "Row wider than Column inner width: {} > 760",
            row_layout.rect.width
        );
    }
}
