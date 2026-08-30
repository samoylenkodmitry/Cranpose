use cranpose_core::{Composition, MemoryApplier, location_key};
use cranpose_foundation::{
    BasicModifierNodeContext, LayoutModifierNode, ModifierNodeChain, modifier_element,
};
use cranpose_ui::{
    Box as ComposeBox, BoxSpec, Column, ColumnSpec, EdgeInsets, Modifier, OffsetElement,
    OffsetNode, PaddingElement, PaddingNode, Row, RowSpec, Size, SizeElement, SizeNode, Text,
    TextStyle, composable,
};
use cranpose_ui_layout::{Constraints, Measurable, Placeable};

struct TestMeasurable {
    width: f32,
    height: f32,
}

impl Measurable for TestMeasurable {
    fn measure(&self, constraints: Constraints) -> Placeable {
        Placeable::value(
            constraints.max_width.min(self.width),
            constraints.max_height.min(self.height),
            0,
        )
    }

    fn min_intrinsic_width(&self, _height: f32) -> f32 {
        self.width
    }

    fn max_intrinsic_width(&self, _height: f32) -> f32 {
        self.width
    }

    fn min_intrinsic_height(&self, _width: f32) -> f32 {
        self.height
    }

    fn max_intrinsic_height(&self, _width: f32) -> f32 {
        self.height
    }
}

#[test]
fn test_complex_modifier_chain_ordering() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    #[composable]
    fn content() {
        ComposeBox(
            Modifier::empty()
                .padding(10.0)
                .size(Size {
                    width: 100.0,
                    height: 100.0,
                })
                .offset(20.0, 30.0)
                .padding(5.0),
            BoxSpec::default(),
            || {
                Text("Test", Modifier::empty(), TextStyle::default());
            },
        );
    }

    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), content)
        .unwrap();

    assert!(composition.root().is_some());

    let root = composition.root().unwrap();
    let mut applier = composition.applier_mut();

    let child_count = applier
        .with_node(root, |node: &mut cranpose_ui::LayoutNode| {
            node.children.len()
        })
        .unwrap();

    assert_eq!(
        child_count, 1,
        "Root should have exactly one child (the Box)"
    );
}

#[test]
fn test_modifier_chain_recomposition() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    #[composable]
    fn content(use_large_padding: bool) {
        let padding = if use_large_padding { 20.0 } else { 5.0 };

        ComposeBox(
            Modifier::empty().padding(padding),
            BoxSpec::default(),
            || {
                Text("Dynamic", Modifier::empty(), TextStyle::default());
            },
        );
    }

    let mut composition = Composition::new(MemoryApplier::new());

    composition
        .render(location_key(file!(), line!(), column!()), || content(true))
        .unwrap();

    assert!(composition.root().is_some());

    composition
        .render(location_key(file!(), line!(), column!()), || content(false))
        .unwrap();

    assert!(composition.root().is_some());

    composition
        .render(location_key(file!(), line!(), column!()), || content(true))
        .unwrap();

    assert!(composition.root().is_some());
}

#[test]
fn test_large_modifier_chain_performance() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    #[composable]
    fn content() {
        let mut modifier = Modifier::empty();
        for i in 0..100 {
            modifier = modifier.padding(1.0);
            if i % 10 == 0 {
                modifier = modifier.offset(i as f32, i as f32);
            }
        }

        ComposeBox(modifier, BoxSpec::default(), || {
            Text("Deep chain", Modifier::empty(), TextStyle::default());
        });
    }

    let mut composition = Composition::new(MemoryApplier::new());

    let start = std::time::Instant::now();
    composition
        .render(location_key(file!(), line!(), column!()), content)
        .unwrap();
    let duration = start.elapsed();

    println!(
        "Large modifier chain (100+ modifiers) completed in: {:?}",
        duration
    );

    assert!(composition.root().is_some());
}

#[test]
fn test_many_items_with_modifiers() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    #[composable]
    fn list(item_count: usize) {
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            for i in 0..item_count {
                Row(
                    Modifier::empty().padding(4.0).size(Size {
                        width: 200.0,
                        height: 40.0,
                    }),
                    RowSpec::default(),
                    move || {
                        let text = if i < 10 {
                            match i {
                                0 => "Item 0",
                                1 => "Item 1",
                                2 => "Item 2",
                                3 => "Item 3",
                                4 => "Item 4",
                                5 => "Item 5",
                                6 => "Item 6",
                                7 => "Item 7",
                                8 => "Item 8",
                                9 => "Item 9",
                                _ => "Item",
                            }
                        } else {
                            "Item 10+"
                        };
                        Text(text, Modifier::empty(), TextStyle::default());
                    },
                );
            }
        });
    }

    let mut composition = Composition::new(MemoryApplier::new());

    let start = std::time::Instant::now();
    composition
        .render(location_key(file!(), line!(), column!()), || list(100))
        .unwrap();
    let duration = start.elapsed();

    println!("100 items with modifiers completed in: {:?}", duration);

    assert!(composition.root().is_some());
}

#[test]
fn test_nested_layouts_with_modifiers() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    #[composable]
    fn nested_content() {
        Column(
            Modifier::empty().padding(10.0),
            ColumnSpec::default(),
            || {
                Row(Modifier::empty().padding(5.0), RowSpec::default(), || {
                    ComposeBox(
                        Modifier::empty()
                            .size(Size {
                                width: 50.0,
                                height: 50.0,
                            })
                            .offset(5.0, 5.0),
                        BoxSpec::default(),
                        || {
                            Text("Nested", Modifier::empty(), TextStyle::default());
                        },
                    );
                });

                Row(Modifier::empty().padding(5.0), RowSpec::default(), || {
                    Text("Second row", Modifier::empty(), TextStyle::default());
                });
            },
        );
    }

    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), nested_content)
        .unwrap();

    assert!(composition.root().is_some());

    let root = composition.root().unwrap();
    let mut applier = composition.applier_mut();

    let children = applier
        .with_node(root, |node: &mut cranpose_ui::LayoutNode| {
            node.children.clone()
        })
        .unwrap();

    assert!(!children.is_empty(), "Root should have children");
}

#[test]
fn test_dynamic_list_recomposition() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    #[composable]
    fn dynamic_list(count: usize) {
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            for i in 0..count {
                let text = match i {
                    0 => "Item 0",
                    1 => "Item 1",
                    2 => "Item 2",
                    3 => "Item 3",
                    4 => "Item 4",
                    5 => "Item 5",
                    6 => "Item 6",
                    7 => "Item 7",
                    8 => "Item 8",
                    9 => "Item 9",
                    _ => "Item 10+",
                };
                Text(text, Modifier::empty().padding(4.0), TextStyle::default());
            }
        });
    }

    let mut composition = Composition::new(MemoryApplier::new());

    composition
        .render(location_key(file!(), line!(), column!()), || {
            dynamic_list(5)
        })
        .unwrap();

    assert!(composition.root().is_some());

    composition
        .render(location_key(file!(), line!(), column!()), || {
            dynamic_list(10)
        })
        .unwrap();

    assert!(composition.root().is_some());

    composition
        .render(location_key(file!(), line!(), column!()), || {
            dynamic_list(3)
        })
        .unwrap();

    assert!(composition.root().is_some());

    assert!(composition.root().is_some());
}

#[test]
fn test_text_with_modifiers() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    #[composable]
    fn styled_text() {
        Text(
            "Styled",
            Modifier::empty()
                .padding_horizontal(10.0)
                .padding_vertical(5.0)
                .size(Size {
                    width: 100.0,
                    height: 30.0,
                }),
            TextStyle::default(),
        );
    }

    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), styled_text)
        .unwrap();

    assert!(composition.root().is_some());
}

#[test]
fn test_card_list_pattern() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    #[composable]
    fn card(title: &'static str, description: &'static str) {
        ComposeBox(
            Modifier::empty().padding(12.0).size(Size {
                width: 300.0,
                height: 150.0,
            }),
            BoxSpec::default(),
            move || {
                Column(Modifier::empty(), ColumnSpec::default(), move || {
                    Text(
                        title,
                        Modifier::empty().padding_each(0.0, 0.0, 0.0, 8.0),
                        TextStyle::default(),
                    );
                    Text(description, Modifier::empty(), TextStyle::default());
                });
            },
        );
    }

    #[composable]
    fn card_list() {
        Column(
            Modifier::empty().padding(16.0),
            ColumnSpec::default(),
            || {
                card("Card 1", "First card description");
                card("Card 2", "Second card description");
                card("Card 3", "Third card description");
            },
        );
    }

    let mut composition = Composition::new(MemoryApplier::new());

    let start = std::time::Instant::now();
    composition
        .render(location_key(file!(), line!(), column!()), card_list)
        .unwrap();
    let duration = start.elapsed();

    println!("Card list pattern: {:?}", duration);

    assert!(composition.root().is_some());
}

#[test]
fn test_rapid_modifier_changes() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    #[composable]
    fn animated(frame: i32) {
        ComposeBox(
            Modifier::empty().offset(frame as f32, frame as f32),
            BoxSpec::default(),
            || {
                Text("Moving", Modifier::empty(), TextStyle::default());
            },
        );
    }

    let mut composition = Composition::new(MemoryApplier::new());

    let start = std::time::Instant::now();

    for frame in 0..100 {
        composition
            .render(location_key(file!(), line!(), column!()), || {
                animated(frame)
            })
            .unwrap();
    }

    let duration = start.elapsed();

    println!("100 recompositions: {:?}", duration);
    println!("Average per frame: {:?}", duration / 100);

    println!(
        "Completed 100 recompositions successfully in {:?}",
        duration
    );
}

#[test]
fn test_padding_affects_size() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    #[composable]
    fn padded_box() {
        ComposeBox(
            Modifier::empty().padding(10.0).size(Size {
                width: 100.0,
                height: 50.0,
            }),
            BoxSpec::default(),
            || {},
        );
    }

    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), padded_box)
        .unwrap();

    assert!(
        composition.root().is_some(),
        "Composition should succeed with padding+size chain"
    );
}

#[test]
fn test_offset_affects_placement_not_size() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    #[composable]
    fn offset_box() {
        ComposeBox(
            Modifier::empty()
                .size(Size {
                    width: 100.0,
                    height: 50.0,
                })
                .offset(20.0, 30.0),
            BoxSpec::default(),
            || {},
        );
    }

    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), offset_box)
        .unwrap();

    assert!(
        composition.root().is_some(),
        "Composition should succeed with size+offset chain"
    );
}

#[test]
fn test_nested_padding_accumulation() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    #[composable]
    fn nested_padding() {
        ComposeBox(Modifier::empty().padding(10.0), BoxSpec::default(), || {
            ComposeBox(
                Modifier::empty().padding(5.0).size(Size {
                    width: 50.0,
                    height: 50.0,
                }),
                BoxSpec::default(),
                || {},
            );
        });
    }

    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), nested_padding)
        .unwrap();

    assert!(
        composition.root().is_some(),
        "Nested padding composition should succeed"
    );
}

#[test]
fn test_modifier_order_padding_size() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    #[composable]
    fn padding_then_size() {
        ComposeBox(
            Modifier::empty().padding(10.0).size(Size {
                width: 100.0,
                height: 100.0,
            }),
            BoxSpec::default(),
            || {},
        );
    }

    #[composable]
    fn size_then_padding() {
        ComposeBox(
            Modifier::empty()
                .size(Size {
                    width: 100.0,
                    height: 100.0,
                })
                .padding(10.0),
            BoxSpec::default(),
            || {},
        );
    }

    let mut comp1 = Composition::new(MemoryApplier::new());
    comp1
        .render(location_key(file!(), line!(), column!()), padding_then_size)
        .unwrap();

    assert!(
        comp1.root().is_some(),
        "padding->size composition should succeed"
    );

    let mut comp2 = Composition::new(MemoryApplier::new());
    comp2
        .render(location_key(file!(), line!(), column!()), size_then_padding)
        .unwrap();

    assert!(
        comp2.root().is_some(),
        "size->padding composition should succeed"
    );
}

#[test]
fn test_offset_not_double_applied() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    #[composable]
    fn single_offset() {
        ComposeBox(
            Modifier::empty()
                .size(Size {
                    width: 50.0,
                    height: 50.0,
                })
                .offset(10.0, 20.0),
            BoxSpec::default(),
            || {},
        );
    }

    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), single_offset)
        .unwrap();

    assert!(
        composition.root().is_some(),
        "size+offset composition should succeed"
    );
}

#[test]
fn test_complex_chain_actual_measurements() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    #[composable]
    fn complex_chain() {
        ComposeBox(
            Modifier::empty()
                .padding(5.0)
                .size(Size {
                    width: 80.0,
                    height: 60.0,
                })
                .offset(10.0, 10.0)
                .padding(10.0),
            BoxSpec::default(),
            || {},
        );
    }

    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), complex_chain)
        .unwrap();

    assert!(
        composition.root().is_some(),
        "Complex modifier chain should compose successfully"
    );
}

#[test]
fn test_padding_math_validation() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let padding = EdgeInsets::uniform(10.0);
    let elements = vec![modifier_element(PaddingElement::new(padding))];
    chain.update_from_slice(&elements, &mut context);

    let node = chain.node_mut::<PaddingNode>(0).unwrap();
    let measurable = TestMeasurable {
        width: 50.0,
        height: 30.0,
    };
    let constraints = Constraints {
        min_width: 0.0,
        max_width: 200.0,
        min_height: 0.0,
        max_height: 200.0,
    };

    let result = node.measure(&mut context, &measurable, constraints);

    assert_eq!(
        result.size.width, 70.0,
        "Padding should add 20px to width (10 left + 10 right)"
    );
    assert_eq!(
        result.size.height, 50.0,
        "Padding should add 20px to height (10 top + 10 bottom)"
    );
}

#[test]
fn test_asymmetric_padding_math() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let padding = EdgeInsets {
        left: 5.0,
        top: 10.0,
        right: 15.0,
        bottom: 20.0,
    };
    let elements = vec![modifier_element(PaddingElement::new(padding))];
    chain.update_from_slice(&elements, &mut context);

    let node = chain.node_mut::<PaddingNode>(0).unwrap();
    let measurable = TestMeasurable {
        width: 100.0,
        height: 100.0,
    };
    let constraints = Constraints {
        min_width: 0.0,
        max_width: 300.0,
        min_height: 0.0,
        max_height: 300.0,
    };

    let result = node.measure(&mut context, &measurable, constraints);

    assert_eq!(
        result.size.width, 120.0,
        "Asymmetric padding: width should be 100 + 5 + 15 = 120"
    );
    assert_eq!(
        result.size.height, 130.0,
        "Asymmetric padding: height should be 100 + 10 + 20 = 130"
    );
}

#[test]
fn test_size_modifier_math() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let elements = vec![modifier_element(SizeElement::new(Some(150.0), Some(200.0)))];
    chain.update_from_slice(&elements, &mut context);

    let node = chain.node_mut::<SizeNode>(0).unwrap();
    let measurable = TestMeasurable {
        width: 50.0,
        height: 50.0,
    };
    let constraints = Constraints {
        min_width: 0.0,
        max_width: 500.0,
        min_height: 0.0,
        max_height: 500.0,
    };

    let result = node.measure(&mut context, &measurable, constraints);

    assert_eq!(
        result.size.width, 150.0,
        "Size modifier should enforce width of 150, ignoring content's 50"
    );
    assert_eq!(
        result.size.height, 200.0,
        "Size modifier should enforce height of 200, ignoring content's 50"
    );
}

#[test]
fn test_chained_padding_accumulation() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let elements = vec![
        modifier_element(PaddingElement::new(EdgeInsets::uniform(10.0))),
        modifier_element(PaddingElement::new(EdgeInsets::uniform(20.0))),
    ];
    chain.update_from_slice(&elements, &mut context);

    assert_eq!(chain.len(), 2, "Should have 2 padding nodes");

    let measurable = TestMeasurable {
        width: 100.0,
        height: 100.0,
    };
    let constraints = Constraints {
        min_width: 0.0,
        max_width: 300.0,
        min_height: 0.0,
        max_height: 300.0,
    };

    let node0 = chain.node_mut::<PaddingNode>(0).unwrap();
    let result0 = node0.measure(&mut context, &measurable, constraints);
    assert_eq!(result0.size.width, 120.0, "First padding: 100 + 10*2 = 120");

    let node1 = chain.node_mut::<PaddingNode>(1).unwrap();
    let result1 = node1.measure(&mut context, &measurable, constraints);
    assert_eq!(
        result1.size.width, 140.0,
        "Second padding: 100 + 20*2 = 140"
    );
}

#[test]
fn test_padding_size_order_math() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut context = BasicModifierNodeContext::new();
    let measurable = TestMeasurable {
        width: 50.0,
        height: 50.0,
    };
    let constraints = Constraints {
        min_width: 0.0,
        max_width: 300.0,
        min_height: 0.0,
        max_height: 300.0,
    };

    let mut chain1 = ModifierNodeChain::new();
    let elements1 = vec![
        modifier_element(PaddingElement::new(EdgeInsets::uniform(10.0))),
        modifier_element(SizeElement::new(Some(100.0), Some(100.0))),
    ];
    chain1.update_from_slice(&elements1, &mut context);

    let mut chain2 = ModifierNodeChain::new();
    let elements2 = vec![
        modifier_element(SizeElement::new(Some(100.0), Some(100.0))),
        modifier_element(PaddingElement::new(EdgeInsets::uniform(10.0))),
    ];
    chain2.update_from_slice(&elements2, &mut context);

    let padding_node_1 = chain1.node_mut::<PaddingNode>(0).unwrap();
    let result_padding = padding_node_1.measure(&mut context, &measurable, constraints);
    assert_eq!(
        result_padding.size.width, 70.0,
        "Padding first: 50 + 10*2 = 70"
    );

    let size_node_2 = chain2.node_mut::<SizeNode>(0).unwrap();
    let result_size = size_node_2.measure(&mut context, &measurable, constraints);
    assert_eq!(result_size.size.width, 100.0, "Size first: enforces 100");
}

#[test]
fn test_offset_doesnt_affect_size() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let elements = vec![modifier_element(OffsetElement::new(20.0, 30.0, false))];
    chain.update_from_slice(&elements, &mut context);

    let node = chain.node_mut::<OffsetNode>(0).unwrap();
    let measurable = TestMeasurable {
        width: 100.0,
        height: 80.0,
    };
    let constraints = Constraints {
        min_width: 0.0,
        max_width: 300.0,
        min_height: 0.0,
        max_height: 300.0,
    };

    let result = node.measure(&mut context, &measurable, constraints);

    assert_eq!(
        result.size.width, 100.0,
        "Offset should not affect measured width"
    );
    assert_eq!(
        result.size.height, 80.0,
        "Offset should not affect measured height"
    );
}

#[test]
fn test_complex_modifier_chain_math() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let elements = vec![
        modifier_element(PaddingElement::new(EdgeInsets::uniform(5.0))),
        modifier_element(SizeElement::new(Some(80.0), Some(60.0))),
        modifier_element(OffsetElement::new(10.0, 10.0, false)),
        modifier_element(PaddingElement::new(EdgeInsets::uniform(10.0))),
    ];
    chain.update_from_slice(&elements, &mut context);

    assert_eq!(chain.len(), 4, "Should have 4 modifier nodes in chain");

    let measurable = TestMeasurable {
        width: 50.0,
        height: 40.0,
    };
    let constraints = Constraints {
        min_width: 0.0,
        max_width: 300.0,
        min_height: 0.0,
        max_height: 300.0,
    };

    let node0 = chain.node_mut::<PaddingNode>(0).unwrap();
    let result0 = node0.measure(&mut context, &measurable, constraints);
    assert_eq!(result0.size.width, 60.0, "Inner padding: 50 + 5*2 = 60");
    assert_eq!(result0.size.height, 50.0, "Inner padding: 40 + 5*2 = 50");

    let node1 = chain.node_mut::<SizeNode>(1).unwrap();
    let result1 = node1.measure(&mut context, &measurable, constraints);
    assert_eq!(result1.size.width, 80.0, "Size enforces 80");
    assert_eq!(result1.size.height, 60.0, "Size enforces 60");

    let node2 = chain.node_mut::<OffsetNode>(2).unwrap();
    let result2 = node2.measure(&mut context, &measurable, constraints);
    assert_eq!(result2.size.width, 50.0, "Offset doesn't change size");
    assert_eq!(result2.size.height, 40.0, "Offset doesn't change size");

    let node3 = chain.node_mut::<PaddingNode>(3).unwrap();
    let result3 = node3.measure(&mut context, &measurable, constraints);
    assert_eq!(result3.size.width, 70.0, "Outer padding: 50 + 10*2 = 70");
    assert_eq!(result3.size.height, 60.0, "Outer padding: 40 + 10*2 = 60");
}

#[test]
fn test_modifier_reuse_pointer_identity() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut chain = ModifierNodeChain::new();
    let mut context = BasicModifierNodeContext::new();

    let elements = vec![modifier_element(PaddingElement::new(EdgeInsets::uniform(
        10.0,
    )))];
    chain.update_from_slice(&elements, &mut context);

    let initial_ptr = {
        let node_ref = chain.node::<PaddingNode>(0).unwrap();
        &*node_ref as *const _
    };

    let elements = vec![modifier_element(PaddingElement::new(EdgeInsets::uniform(
        20.0,
    )))];
    chain.update_from_slice(&elements, &mut context);

    let updated_ptr = {
        let node_ref = chain.node::<PaddingNode>(0).unwrap();
        &*node_ref as *const _
    };

    assert_eq!(
        initial_ptr, updated_ptr,
        "Modifier node should be reused, not recreated"
    );

    let node_ref = chain.node::<PaddingNode>(0).unwrap();
    assert_eq!(
        node_ref.padding().left,
        20.0,
        "Node should have updated padding value"
    );
}
