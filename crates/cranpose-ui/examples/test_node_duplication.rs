use cranpose_core::{location_key, Composition, MemoryApplier};
use cranpose_ui::{composable, Column, ColumnSpec, Modifier, Text, TextStyle};

#[composable]
fn inner_content() {
    Text("Inner 1", Modifier::empty(), TextStyle::default());
    Text("Inner 2", Modifier::empty(), TextStyle::default());
}

#[composable]
fn outer_content() {
    Text("Outer start", Modifier::empty(), TextStyle::default());

    // This Column should contain "Inner 1" and "Inner 2"
    Column(Modifier::empty(), ColumnSpec::default(), || {
        inner_content();
    });

    Text("Outer end", Modifier::empty(), TextStyle::default());
}

fn main() {
    let mut composition = Composition::new(MemoryApplier::new());

    // Initial render
    composition
        .render(location_key(file!(), line!(), column!()), outer_content)
        .unwrap();

    if let Some(root) = composition.root() {
        let mut applier = composition.applier_mut();
        println!("Root node #{} children:", root);
        if let Ok(root_node) = applier.with_node(root, |node: &mut cranpose_ui::LayoutNode| {
            println!("  Children: {:?}", node.children);
            node.children.clone()
        }) {
            for child_id in root_node {
                println!("  Child #{}", child_id);
                if let Ok(child_children) = applier
                    .with_node(child_id, |node: &mut cranpose_ui::LayoutNode| {
                        node.children.clone()
                    })
                {
                    for grandchild in child_children {
                        println!("    Grandchild #{}", grandchild);
                    }
                }
            }
        }
    }
}
