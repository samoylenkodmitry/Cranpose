use cranpose_core::MutableState;
use cranpose_testing::ComposeTestRule;
use cranpose_ui::{HeadlessRenderer, LayoutBox, LayoutEngine, LayoutTree, RenderOp, Size};
use desktop_app::app::{
    complex_chain_showcase, dynamic_modifiers_showcase, item_list_showcase,
    positioned_boxes_showcase, simple_card_showcase,
};

fn compute_layout_from_rule(
    rule: &mut ComposeTestRule,
    max_width: f32,
    max_height: f32,
) -> Result<LayoutTree, cranpose_core::NodeError> {
    let root = rule.root_id().expect("should have root");
    let handle = rule.runtime_handle();

    let layout = {
        let mut applier = rule.applier_mut();
        applier.set_runtime_handle(handle);
        let result = applier.compute_layout(
            root,
            Size {
                width: max_width,
                height: max_height,
            },
        )?;
        applier.clear_runtime_handle();
        result
    };

    Ok(layout)
}

fn dump_layout_tree(layout_box: &LayoutBox, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    let rect = &layout_box.rect;

    let text_content = layout_box
        .node_data
        .modifier_slices()
        .text_content()
        .map(|s| format!(" text=\"{}\"", s))
        .unwrap_or_default();

    let mut output = format!(
        "{}[{}] pos=({:.1}, {:.1}) size=({:.1}x{:.1}){}\n",
        indent, layout_box.node_id, rect.x, rect.y, rect.width, rect.height, text_content
    );

    for child in &layout_box.children {
        output.push_str(&dump_layout_tree(child, depth + 1));
    }

    output
}

fn validate_child_within_parent(parent: &LayoutBox, child: &LayoutBox) -> Result<(), String> {
    let tolerance = 1.0;

    if child.rect.x + tolerance < parent.rect.x || child.rect.y + tolerance < parent.rect.y {
        return Err(format!(
            "Child {} base origin ({:.1},{:.1}) outside parent {} at ({:.1},{:.1}) size ({:.1}x{:.1})",
            child.node_id, child.rect.x, child.rect.y,
            parent.node_id, parent.rect.x, parent.rect.y, parent.rect.width, parent.rect.height
        ));
    }

    let has_explicit_offset = child.node_data.resolved_modifiers().offset().x.abs() > 0.001
        || child.node_data.resolved_modifiers().offset().y.abs() > 0.001;

    if !has_explicit_offset
        && (child.rect.x + child.rect.width > parent.rect.x + parent.rect.width + tolerance
            || child.rect.y + child.rect.height > parent.rect.y + parent.rect.height + tolerance)
    {
        return Err(format!(
            "Child {} at ({:.1},{:.1}) size ({:.1}x{:.1}) outside parent {} at ({:.1},{:.1}) size ({:.1}x{:.1})",
            child.node_id, child.rect.x, child.rect.y, child.rect.width, child.rect.height,
            parent.node_id, parent.rect.x, parent.rect.y, parent.rect.width, parent.rect.height
        ));
    }

    Ok(())
}

fn validate_layout_hierarchy(layout_box: &LayoutBox) -> Result<(), String> {
    for child in &layout_box.children {
        validate_child_within_parent(layout_box, child)?;
        validate_layout_hierarchy(child)?;
    }
    Ok(())
}

fn find_box_with_text<'a>(layout_box: &'a LayoutBox, text: &str) -> Option<&'a LayoutBox> {
    if let Some(content) = layout_box.node_data.modifier_slices().text_content() {
        if content == text {
            return Some(layout_box);
        }
    }

    for child in &layout_box.children {
        if let Some(found) = find_box_with_text(child, text) {
            return Some(found);
        }
    }

    None
}

fn collect_all_boxes<'a>(layout_box: &'a LayoutBox, result: &mut Vec<&'a LayoutBox>) {
    result.push(layout_box);
    for child in &layout_box.children {
        collect_all_boxes(child, result);
    }
}

#[test]
fn test_simple_card_layout_positions() {
    let mut rule = ComposeTestRule::new();
    rule.set_content(|| {
        simple_card_showcase();
    })
    .expect("Simple card should render");

    let layout = compute_layout_from_rule(&mut rule, 800.0, 600.0).expect("Should compute layout");

    println!("=== Simple Card Layout ===");
    println!("{}", dump_layout_tree(layout.root(), 0));

    validate_layout_hierarchy(layout.root()).expect("Layout hierarchy should be valid");

    let title = find_box_with_text(layout.root(), "Card Title").expect("Should find Card Title");
    let description = find_box_with_text(layout.root(), "Card content goes here with padding")
        .expect("Should find description");
    let action1 =
        find_box_with_text(layout.root(), "Action 1").expect("Should find Action 1 button");
    let action2 =
        find_box_with_text(layout.root(), "Action 2").expect("Should find Action 2 button");

    assert!(
        title.rect.y < description.rect.y,
        "Title should be above description: title.y={:.2} vs desc.y={:.2}",
        title.rect.y,
        description.rect.y
    );

    assert!(
        description.rect.y < action1.rect.y,
        "Description should be above Action 1: desc.y={:.2} vs action1.y={:.2}",
        description.rect.y,
        action1.rect.y
    );

    assert!(
        (action1.rect.y - action2.rect.y).abs() < 2.0,
        "Action buttons should be at same vertical position: action1.y={:.2} vs action2.y={:.2}",
        action1.rect.y,
        action2.rect.y
    );

    assert!(
        action1.rect.x < action2.rect.x,
        "Action 1 should be left of Action 2: action1.x={:.2} vs action2.x={:.2}",
        action1.rect.x,
        action2.rect.x
    );

    println!("✓ Simple card with border and action buttons layout is correct");
}

#[test]
fn test_positioned_boxes_layout() {
    let mut rule = ComposeTestRule::new();
    rule.set_content(|| {
        positioned_boxes_showcase();
    })
    .expect("Positioned boxes should render");

    let layout = compute_layout_from_rule(&mut rule, 800.0, 600.0).expect("Should compute layout");

    println!("=== Positioned Boxes Layout ===");
    println!("{}", dump_layout_tree(layout.root(), 0));

    validate_layout_hierarchy(layout.root()).expect("Layout hierarchy should be valid");

    let box_a = find_box_with_text(layout.root(), "Box A").expect("Should find Box A");
    let box_b = find_box_with_text(layout.root(), "Box B").expect("Should find Box B");
    let box_c = find_box_with_text(layout.root(), "C").expect("Should find Box C");
    let box_d = find_box_with_text(layout.root(), "Box D").expect("Should find Box D");

    println!(
        "Box A: pos=({:.1}, {:.1}) size=({:.1}x{:.1})",
        box_a.rect.x, box_a.rect.y, box_a.rect.width, box_a.rect.height
    );
    println!(
        "Box B: pos=({:.1}, {:.1}) size=({:.1}x{:.1})",
        box_b.rect.x, box_b.rect.y, box_b.rect.width, box_b.rect.height
    );
    println!(
        "Box C: pos=({:.1}, {:.1}) size=({:.1}x{:.1})",
        box_c.rect.x, box_c.rect.y, box_c.rect.width, box_c.rect.height
    );
    println!(
        "Box D: pos=({:.1}, {:.1}) size=({:.1}x{:.1})",
        box_d.rect.x, box_d.rect.y, box_d.rect.width, box_d.rect.height
    );

    assert!(box_a.rect.x < box_b.rect.x, "Box A should be left of Box B");
    assert!(box_a.rect.y < box_b.rect.y, "Box A should be above Box B");

    assert!(box_c.rect.y < box_d.rect.y, "Box C should be above Box D");

    assert!(
        box_b.rect.x > box_a.rect.x && box_b.rect.x > box_c.rect.x && box_b.rect.x > box_d.rect.x,
        "Box B should be rightmost"
    );

    assert!(
        box_a.rect.y <= box_b.rect.y
            && box_a.rect.y <= box_c.rect.y
            && box_a.rect.y <= box_d.rect.y,
        "Box A should be topmost or equal"
    );

    println!("✓ Positioned boxes (4 boxes with different sizes) layout is correct");
}

#[test]
fn test_item_list_spacing() {
    let mut rule = ComposeTestRule::new();
    rule.set_content(|| {
        item_list_showcase();
    })
    .expect("Item list should render");

    let layout = compute_layout_from_rule(&mut rule, 800.0, 600.0).expect("Should compute layout");

    println!("=== Item List Layout ===");
    println!("{}", dump_layout_tree(layout.root(), 0));

    validate_layout_hierarchy(layout.root()).expect("Layout hierarchy should be valid");

    let item1 = find_box_with_text(layout.root(), "Item #0").expect("Should find Item #0");
    let item2 = find_box_with_text(layout.root(), "Item #1").expect("Should find Item #1");
    let item3 = find_box_with_text(layout.root(), "Item #2").expect("Should find Item #2");

    assert!(item1.rect.y < item2.rect.y, "Item 1 should be above Item 2");
    assert!(item2.rect.y < item3.rect.y, "Item 2 should be above Item 3");

    let spacing_1_2 = item2.rect.y - (item1.rect.y + item1.rect.height);
    let spacing_2_3 = item3.rect.y - (item2.rect.y + item2.rect.height);

    println!("Spacing between Item 1 and 2: {:.1}", spacing_1_2);
    println!("Spacing between Item 2 and 3: {:.1}", spacing_2_3);

    let spacing_diff = (spacing_1_2 - spacing_2_3).abs();
    assert!(
        spacing_diff < 1.0,
        "Item spacing should be consistent: {:.2} vs {:.2} (diff: {:.2})",
        spacing_1_2,
        spacing_2_3,
        spacing_diff
    );

    let renderer = HeadlessRenderer::new();
    let scene = renderer.render(&layout);

    let background_count = scene
        .operations()
        .iter()
        .filter(|op| matches!(op, RenderOp::Primitive { .. }))
        .count();

    println!("Background primitives found: {}", background_count);
    assert!(
        background_count >= 16,
        "Should have at least 16 background primitives (title + 5 items with borders and status), got {}",
        background_count
    );

    println!("✓ Item list with alternating colors, borders, and status indicators is correct");
}

#[test]
fn test_complex_chain_modifier_ordering() {
    let mut rule = ComposeTestRule::new();
    rule.set_content(|| {
        complex_chain_showcase();
    })
    .expect("Complex chain should render");

    let layout = compute_layout_from_rule(&mut rule, 800.0, 600.0).expect("Should compute layout");

    println!("=== Complex Chain Layout ===");
    println!("{}", dump_layout_tree(layout.root(), 0));

    validate_layout_hierarchy(layout.root()).expect("Layout hierarchy should be valid");

    let renderer = HeadlessRenderer::new();
    let scene = renderer.render(&layout);

    println!("\nRender operations: {}", scene.operations().len());
    let mut background_count = 0;
    let mut text_count = 0;

    for (i, op) in scene.operations().iter().enumerate() {
        match op {
            RenderOp::Primitive {
                node_id,
                layer,
                primitive: _,
            } => {
                println!("  [{}] Primitive node={} layer={:?}", i, node_id, layer);
                background_count += 1;
            }
            RenderOp::Text {
                node_id,
                rect,
                value,
            } => {
                println!(
                    "  [{}] Text node={} pos=({:.1},{:.1}) \"{}\"",
                    i, node_id, rect.x, rect.y, value
                );
                text_count += 1;
            }
        }
    }

    println!("\nBackground primitives: {}", background_count);
    println!("Text elements: {}", text_count);

    assert_eq!(
        background_count, 6,
        "Should have 6 background primitives for nested box structure, got {}",
        background_count
    );

    assert_eq!(
        text_count, 5,
        "Should have 5 text elements, got {}",
        text_count
    );

    let nested_text =
        find_box_with_text(layout.root(), "Nested!").expect("Should find 'Nested!' text");
    let offset_text = find_box_with_text(layout.root(), "Offset + Sized")
        .expect("Should find 'Offset + Sized' text");

    println!(
        "\nNested! text at: ({:.1}, {:.1}) size=({:.1}x{:.1})",
        nested_text.rect.x, nested_text.rect.y, nested_text.rect.width, nested_text.rect.height
    );
    println!(
        "Offset + Sized text at: ({:.1}, {:.1}) size=({:.1}x{:.1})",
        offset_text.rect.x, offset_text.rect.y, offset_text.rect.width, offset_text.rect.height
    );

    assert!(
        offset_text.rect.x >= 20.0,
        "Offset box should be offset by at least 20px, got x={:.1}",
        offset_text.rect.x
    );

    println!("✓ Complex chain with nested backgrounds renders correctly");
}

#[test]
fn test_dynamic_modifiers_size_changes() {
    let mut rule = ComposeTestRule::new();

    rule.set_content(|| {
        dynamic_modifiers_showcase();
    })
    .expect("Dynamic modifiers should render");

    let layout = compute_layout_from_rule(&mut rule, 800.0, 600.0).expect("Should compute layout");

    println!("=== Dynamic Modifiers (Initial) ===");
    println!("{}", dump_layout_tree(layout.root(), 0));

    validate_layout_hierarchy(layout.root()).expect("Layout hierarchy should be valid at frame 0");

    println!("✓ Dynamic modifiers render correctly");
}

#[test]
fn test_dynamic_modifiers_frame_advancement() {
    let mut rule = ComposeTestRule::new();
    let frame = MutableState::with_runtime(0i32, rule.runtime_handle());

    rule.set_content({
        move || {
            let frame_inner = frame;
            dynamic_modifiers_showcase_with_frame(frame_inner);
        }
    })
    .expect("Dynamic modifiers should render");

    let layout0 = compute_layout_from_rule(&mut rule, 800.0, 600.0)
        .expect("Should compute layout at frame 0");
    println!("\n=== Dynamic Modifiers Frame 0 ===");
    println!("{}", dump_layout_tree(layout0.root(), 0));
    validate_layout_hierarchy(layout0.root()).expect("Layout should be valid at frame 0");

    frame.set(5);
    rule.pump_until_idle()
        .expect("Should recompose after frame advance");
    let layout5 = compute_layout_from_rule(&mut rule, 800.0, 600.0)
        .expect("Should compute layout at frame 5");
    println!("\n=== Dynamic Modifiers Frame 5 (x=50) ===");
    println!("{}", dump_layout_tree(layout5.root(), 0));
    validate_layout_hierarchy(layout5.root()).expect("Layout should be valid at frame 5");

    frame.set(10);
    rule.pump_until_idle()
        .expect("Should recompose after frame advance");
    let layout10 = compute_layout_from_rule(&mut rule, 800.0, 600.0)
        .expect("Should compute layout at frame 10");
    println!("\n=== Dynamic Modifiers Frame 10 (x=100) ===");
    println!("{}", dump_layout_tree(layout10.root(), 0));
    validate_layout_hierarchy(layout10.root()).expect("Layout should be valid at frame 10");

    frame.set(19);
    rule.pump_until_idle()
        .expect("Should recompose after frame advance");
    let layout19 = compute_layout_from_rule(&mut rule, 800.0, 600.0)
        .expect("Should compute layout at frame 19");
    println!("\n=== Dynamic Modifiers Frame 19 (x=190) ===");
    println!("{}", dump_layout_tree(layout19.root(), 0));

    match validate_layout_hierarchy(layout19.root()) {
        Ok(_) => println!("✓ Layout valid at frame 19"),
        Err(e) => {
            println!("✗ BUG FOUND at frame 19: {}", e);
            panic!("Layout hierarchy validation failed at frame 19: {}", e);
        }
    }

    println!("✓ Dynamic modifiers handle frame advancement correctly");
}

fn dynamic_modifiers_showcase_with_frame(frame: MutableState<i32>) {
    use cranpose_ui::*;

    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(
            "=== Dynamic Modifiers ===",
            Modifier::empty()
                .padding(12.0)
                .then(Modifier::empty().background(Color(1.0, 1.0, 1.0, 0.1)))
                .then(Modifier::empty().rounded_corners(14.0)),
            TextStyle::default(),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        let current_frame = frame.get();
        let x = (current_frame as f32 * 10.0) % 200.0;
        let y = 50.0;

        cranpose_ui::Box(
            Modifier::empty()
                .size_points(250.0, 150.0)
                .then(Modifier::empty().background(Color(0.05, 0.05, 0.15, 0.5)))
                .then(Modifier::empty().rounded_corners(8.0)),
            BoxSpec::default(),
            move || {
                cranpose_ui::Box(
                    Modifier::empty()
                        .size(Size {
                            width: 50.0,
                            height: 50.0,
                        })
                        .then(Modifier::empty().offset(x, y))
                        .then(Modifier::empty().padding(6.0))
                        .then(Modifier::empty().background(Color(0.3, 0.6, 0.9, 0.9)))
                        .then(Modifier::empty().rounded_corners(10.0)),
                    BoxSpec::default(),
                    || {
                        Text("Move", Modifier::empty(), TextStyle::default());
                    },
                );
            },
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        Text(
            format!("Frame: {}, X: {:.1}", current_frame, x),
            Modifier::empty()
                .padding(8.0)
                .then(Modifier::empty().background(Color(0.2, 0.2, 0.3, 0.6)))
                .then(Modifier::empty().rounded_corners(10.0)),
            TextStyle::default(),
        );
    });
}

#[test]
fn test_all_showcases_have_valid_layouts() {
    let showcases = vec![
        ("Simple Card", simple_card_showcase as fn()),
        ("Positioned Boxes", positioned_boxes_showcase as fn()),
        ("Item List", item_list_showcase as fn()),
        ("Complex Chain", complex_chain_showcase as fn()),
    ];

    for (name, showcase_fn) in showcases {
        println!("\n=== Testing {} ===", name);

        let mut rule = ComposeTestRule::new();
        rule.set_content(showcase_fn)
            .unwrap_or_else(|_| panic!("{} should render", name));

        let layout = compute_layout_from_rule(&mut rule, 800.0, 600.0)
            .unwrap_or_else(|_| panic!("{} should compute layout", name));

        validate_layout_hierarchy(layout.root())
            .unwrap_or_else(|_| panic!("{} layout hierarchy should be valid", name));

        assert!(
            layout.root().rect.width > 0.0,
            "{} root should have width",
            name
        );
        assert!(
            layout.root().rect.height > 0.0,
            "{} root should have height",
            name
        );

        let mut all_boxes = Vec::new();
        collect_all_boxes(layout.root(), &mut all_boxes);

        for box_ref in &all_boxes {
            assert!(
                box_ref.rect.x >= -0.01,
                "{}: Box [{}] has negative x: {:.2}",
                name,
                box_ref.node_id,
                box_ref.rect.x
            );
            assert!(
                box_ref.rect.y >= -0.01,
                "{}: Box [{}] has negative y: {:.2}",
                name,
                box_ref.node_id,
                box_ref.rect.y
            );
            assert!(
                box_ref.rect.width >= 0.0,
                "{}: Box [{}] has negative width: {:.2}",
                name,
                box_ref.node_id,
                box_ref.rect.width
            );
            assert!(
                box_ref.rect.height >= 0.0,
                "{}: Box [{}] has negative height: {:.2}",
                name,
                box_ref.node_id,
                box_ref.rect.height
            );
        }

        println!("✓ {} has valid layout ({} boxes)", name, all_boxes.len());
    }
}
