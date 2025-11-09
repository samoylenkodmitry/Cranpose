use super::{
    Color, ComposeModifier, DimensionConstraint, EdgeInsets, InspectableModifier, InspectorInfo,
    Modifier, ModifierChainHandle, Point,
};
use crate::modifier_nodes::{AlphaNode, BackgroundNode, ClickableNode, PaddingNode};
use std::any::TypeId;

#[test]
fn padding_nodes_resolve_padding_values() {
    let modifier = Modifier::padding(4.0)
        .then(Modifier::padding_horizontal(2.0))
        .then(Modifier::padding_each(1.0, 3.0, 5.0, 7.0));
    let mut handle = ModifierChainHandle::new();
    handle.update(&modifier);
    let padding = handle.resolved_modifiers().padding();
    assert_eq!(
        padding,
        EdgeInsets {
            left: 7.0,
            top: 7.0,
            right: 11.0,
            bottom: 11.0,
        }
    );
}

#[test]
fn fill_max_size_sets_fraction_constraints() {
    let modifier = Modifier::fill_max_size_fraction(0.75);
    let props = modifier.layout_properties();
    assert_eq!(props.width(), DimensionConstraint::Fraction(0.75));
    assert_eq!(props.height(), DimensionConstraint::Fraction(0.75));
}

#[test]
fn weight_tracks_fill_flag() {
    let modifier = Modifier::weight_with_fill(2.0, false);
    let props = modifier.layout_properties();
    let weight = props.weight().expect("weight to be recorded");
    assert_eq!(weight.weight, 2.0);
    assert!(!weight.fill);
}

#[test]
fn offset_accumulates_across_chain() {
    let modifier = Modifier::offset(4.0, 6.0)
        .then(Modifier::absolute_offset(-1.5, 2.5))
        .then(Modifier::offset(0.5, -3.0));
    let total = modifier.total_offset();
    assert_eq!(total, Point { x: 3.0, y: 5.5 });
}

#[test]
fn fold_in_iterates_in_insertion_order() {
    let modifier = Modifier::padding(2.0)
        .then(Modifier::background(Color(0.1, 0.2, 0.3, 1.0)))
        .then(Modifier::clickable(|_| {}));

    let node_types = modifier.fold_in(Vec::new(), |mut acc, element| {
        acc.push(element.node_type());
        acc
    });

    assert_eq!(
        node_types,
        vec![
            TypeId::of::<PaddingNode>(),
            TypeId::of::<BackgroundNode>(),
            TypeId::of::<ClickableNode>(),
        ]
    );
}

#[test]
fn fold_out_iterates_in_reverse_order() {
    let modifier = Modifier::padding(2.0)
        .then(Modifier::background(Color(0.1, 0.2, 0.3, 1.0)))
        .then(Modifier::clickable(|_| {}));

    let node_types = modifier.fold_out(Vec::new(), |mut acc, element| {
        acc.push(element.node_type());
        acc
    });

    assert_eq!(
        node_types,
        vec![
            TypeId::of::<ClickableNode>(),
            TypeId::of::<BackgroundNode>(),
            TypeId::of::<PaddingNode>(),
        ]
    );
}

#[test]
fn any_and_all_respect_predicates() {
    let modifier = Modifier::padding(2.0)
        .then(Modifier::background(Color(0.1, 0.2, 0.3, 1.0)))
        .then(Modifier::clickable(|_| {}));

    assert!(modifier.any(|element| element.node_type() == TypeId::of::<BackgroundNode>()));
    assert!(!modifier.any(|element| element.node_type() == TypeId::of::<AlphaNode>()));

    assert!(modifier.all(|element| element.node_type() != TypeId::of::<AlphaNode>()));
    assert!(Modifier::empty().all(|_| false));
}

#[test]
fn then_short_circuits_empty_modifiers() {
    let padding = Modifier::padding(4.0);
    assert_eq!(Modifier::empty().then(padding.clone()), padding);

    let background = Modifier::background(Color::rgba(0.2, 0.4, 0.6, 1.0));
    assert_eq!(background.then(Modifier::empty()), background);
}

#[test]
fn then_preserves_element_order_when_chaining() {
    let modifier = Modifier::empty()
        .then(Modifier::padding(2.0))
        .then(Modifier::background(Color(0.1, 0.2, 0.3, 1.0)))
        .then(Modifier::clickable(|_| {}));

    let node_types = modifier.fold_in(Vec::new(), |mut acc, element| {
        acc.push(element.node_type());
        acc
    });

    assert_eq!(
        node_types,
        vec![
            TypeId::of::<PaddingNode>(),
            TypeId::of::<BackgroundNode>(),
            TypeId::of::<ClickableNode>(),
        ]
    );
}

#[test]
fn inspector_metadata_records_padding_and_background() {
    let modifier = Modifier::padding_each(4.0, 2.0, 1.0, 3.0)
        .then(Modifier::background(Color::rgba(0.8, 0.1, 0.2, 1.0)));

    let mut info = InspectorInfo::new();
    modifier.inspect(&mut info);
    let props = info.properties();

    let expected_left = 4.0.to_string();
    assert!(props
        .iter()
        .any(|prop| prop.name == "paddingLeft" && prop.value == expected_left));

    let expected_color = format!("{:?}", Color::rgba(0.8, 0.1, 0.2, 1.0));
    assert!(props
        .iter()
        .any(|prop| prop.name == "backgroundColor" && prop.value == expected_color));
}

#[test]
fn inspector_metadata_records_size_and_clickable() {
    let modifier = Modifier::size_points(24.0, 48.0).then(Modifier::clickable(|_| {}));

    let mut info = InspectorInfo::new();
    modifier.inspect(&mut info);
    let props = info.properties();

    assert!(props
        .iter()
        .any(|prop| prop.name == "width" && prop.value == 24.0f32.to_string()));
    assert!(props
        .iter()
        .any(|prop| prop.name == "height" && prop.value == 48.0f32.to_string()));
    assert!(props
        .iter()
        .any(|prop| prop.name == "onClick" && prop.value == "provided"));
}

#[test]
fn inspector_metadata_preserves_modifier_order() {
    let modifier = Modifier::width(16.0)
        .then(Modifier::fill_max_height_fraction(0.5))
        .then(Modifier::clip_to_bounds());

    let mut info = InspectorInfo::new();
    modifier.inspect(&mut info);
    let names: Vec<&'static str> = info.properties().iter().map(|prop| prop.name).collect();
    assert_eq!(names, vec!["width", "height", "clipToBounds"]);
}

#[test]
fn inspector_debug_helpers_surface_properties() {
    let modifier = Modifier::offset(2.0, -1.0).then(Modifier::clip_to_bounds());

    let mut info = InspectorInfo::new();
    modifier.inspect(&mut info);

    let description = info.describe();
    assert!(description.contains("offsetX=2"));
    assert!(description.contains("offsetY=-1"));
    assert!(description.contains("clipToBounds=true"));

    let debug_pairs = info.debug_properties();
    assert_eq!(
        debug_pairs,
        vec![
            ("offsetX", 2.0f32.to_string()),
            ("offsetY", (-1.0f32).to_string()),
            ("clipToBounds", "true".to_string())
        ]
    );
}
