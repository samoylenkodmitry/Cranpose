use super::{
    modifier_local_of, Alignment, Color, ComposeModifier, DimensionConstraint, EdgeInsets,
    GraphicsLayer, HorizontalAlignment, InspectableModifier, InspectorInfo, Modifier,
    ModifierChainHandle, ModifierLocalSource, ModifierLocalToken, Point, Size, VerticalAlignment,
};
use crate::modifier_nodes::{AlphaNode, BackgroundNode, ClickableNode, PaddingNode};
use std::any::TypeId;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn padding_nodes_resolve_padding_values() {
    let modifier = Modifier::padding(4.0)
        .then(Modifier::padding_horizontal(2.0))
        .then(Modifier::padding_each(1.0, 3.0, 5.0, 7.0));
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);
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
    let props = modifier.resolved_modifiers().layout_properties();
    assert_eq!(props.width(), DimensionConstraint::Fraction(0.75));
    assert_eq!(props.height(), DimensionConstraint::Fraction(0.75));
}

#[test]
fn weight_tracks_fill_flag() {
    let modifier = Modifier::weight_with_fill(2.0, false);
    let props = modifier.resolved_modifiers().layout_properties();
    let weight = props.weight().expect("weight to be recorded");
    assert_eq!(weight.weight, 2.0);
    assert!(!weight.fill);
}

#[test]
fn offset_accumulates_across_chain() {
    let modifier = Modifier::offset(4.0, 6.0)
        .then(Modifier::absolute_offset(-1.5, 2.5))
        .then(Modifier::offset(0.5, -3.0));
    let total = modifier.resolved_modifiers().offset();
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

    let expected = vec![
        TypeId::of::<PaddingNode>(),
        TypeId::of::<BackgroundNode>(),
        TypeId::of::<ClickableNode>(),
    ];
    assert!(
        node_types.len() >= expected.len(),
        "modifier chain missing expected elements"
    );
    assert_eq!(&node_types[..expected.len()], expected);
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

    let expected = vec![
        TypeId::of::<ClickableNode>(),
        TypeId::of::<BackgroundNode>(),
        TypeId::of::<PaddingNode>(),
    ];
    assert!(
        node_types.len() >= expected.len(),
        "modifier chain missing expected elements"
    );
    let start = node_types.len() - expected.len();
    assert_eq!(&node_types[start..], expected);
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

    let expected = vec![
        TypeId::of::<PaddingNode>(),
        TypeId::of::<BackgroundNode>(),
        TypeId::of::<ClickableNode>(),
    ];
    assert!(
        node_types.len() >= expected.len(),
        "modifier chain missing expected elements"
    );
    assert_eq!(&node_types[..expected.len()], expected);
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
fn required_size_sets_explicit_constraints() {
    let modifier = Modifier::required_size(Size {
        width: 32.0,
        height: 18.0,
    });
    let props = modifier.resolved_modifiers().layout_properties();
    assert_eq!(props.width(), DimensionConstraint::Points(32.0));
    assert_eq!(props.height(), DimensionConstraint::Points(18.0));
    assert_eq!(props.min_width(), Some(32.0));
    assert_eq!(props.max_width(), Some(32.0));
    assert_eq!(props.min_height(), Some(18.0));
    assert_eq!(props.max_height(), Some(18.0));
}

#[test]
fn alignment_modifiers_record_values() {
    let modifier = Modifier::align(Alignment::BOTTOM_END)
        .alignInColumn(HorizontalAlignment::CenterHorizontally)
        .alignInRow(VerticalAlignment::Top);
    let props = modifier.resolved_modifiers().layout_properties();
    assert_eq!(props.box_alignment(), Some(Alignment::BOTTOM_END));
    assert_eq!(
        props.column_alignment(),
        Some(HorizontalAlignment::CenterHorizontally)
    );
    assert_eq!(props.row_alignment(), Some(VerticalAlignment::Top));
}

#[test]
fn graphics_layer_modifier_updates_resolved_layer() {
    let layer = GraphicsLayer {
        alpha: 0.5,
        ..Default::default()
    };
    let modifier = Modifier::graphics_layer(layer);
    assert_eq!(modifier.graphics_layer_values(), Some(layer));
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

#[test]
fn modifier_local_consumer_reads_provided_value() {
    let key = modifier_local_of(|| 0);
    let observed = Rc::new(RefCell::new(None));
    let key_clone = key.clone();
    let capture = observed.clone();

    let modifier = Modifier::empty()
        .modifier_local_provider(key, || 42)
        .modifier_local_consumer(move |scope| {
            capture.borrow_mut().replace(*scope.get(&key_clone));
        });

    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    assert_eq!(observed.borrow().as_ref(), Some(&42));
}

#[test]
fn modifier_local_consumer_uses_default_when_missing() {
    let key = modifier_local_of(|| String::from("fallback"));
    let observed = Rc::new(RefCell::new(None));
    let key_clone = key.clone();
    let capture = observed.clone();

    let modifier = Modifier::empty().modifier_local_consumer(move |scope| {
        capture.borrow_mut().replace(scope.get(&key_clone).clone());
    });

    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    assert_eq!(observed.borrow().as_ref(), Some(&String::from("fallback")));
}

#[test]
fn modifier_local_consumer_runs_only_when_dependencies_change() {
    let key = modifier_local_of(|| 0);
    let observed = Rc::new(RefCell::new(Vec::new()));
    let capture = observed.clone();
    let key_clone = key.clone();

    let modifier = Modifier::empty()
        .modifier_local_provider(key.clone(), || 42)
        .modifier_local_consumer(move |scope| {
            capture.borrow_mut().push(*scope.get(&key_clone));
        });

    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);
    let _ = handle.update(&modifier);

    let values = observed.borrow();
    assert_eq!(values.as_slice(), &[42]);
}

#[test]
fn modifier_local_consumer_reads_from_parent_chain() {
    let key = modifier_local_of(|| 0);
    let observed = Rc::new(RefCell::new(Vec::new()));
    let capture = observed.clone();
    let key_clone = key.clone();

    let mut parent_handle = ModifierChainHandle::new();
    let parent_modifier = Modifier::empty().modifier_local_provider(key.clone(), || 7);
    let _ = parent_handle.update(&parent_modifier);

    let child_modifier = Modifier::empty().modifier_local_consumer(move |scope| {
        capture.borrow_mut().push(*scope.get(&key_clone));
    });
    let mut child_handle = ModifierChainHandle::new();
    {
        let mut resolver = |token: ModifierLocalToken| {
            parent_handle
                .resolve_modifier_local(token)
                .map(|value| value.with_source(ModifierLocalSource::Ancestor))
        };
        let _ = child_handle.update_with_resolver(&child_modifier, &mut resolver);
    }

    assert_eq!(observed.borrow().as_slice(), &[7]);
}

#[test]
fn modifier_local_consumer_invalidated_by_parent_change() {
    let key = modifier_local_of(|| 0);
    let observed = Rc::new(RefCell::new(Vec::new()));
    let capture = observed.clone();
    let key_clone = key.clone();

    let mut parent_handle = ModifierChainHandle::new();
    let mut child_handle = ModifierChainHandle::new();

    let child_modifier = Modifier::empty().modifier_local_consumer(move |scope| {
        capture.borrow_mut().push(*scope.get(&key_clone));
    });

    let _ = parent_handle.update(&Modifier::empty().modifier_local_provider(key.clone(), || 1));
    {
        let mut resolver = |token: ModifierLocalToken| {
            parent_handle
                .resolve_modifier_local(token)
                .map(|value| value.with_source(ModifierLocalSource::Ancestor))
        };
        let _ = child_handle.update_with_resolver(&child_modifier, &mut resolver);
    }

    let _ = parent_handle.update(&Modifier::empty().modifier_local_provider(key.clone(), || 5));
    {
        let mut resolver = |token: ModifierLocalToken| {
            parent_handle
                .resolve_modifier_local(token)
                .map(|value| value.with_source(ModifierLocalSource::Ancestor))
        };
        let _ = child_handle.update_with_resolver(&child_modifier, &mut resolver);
    }

    assert_eq!(observed.borrow().as_slice(), &[1, 5]);
}
