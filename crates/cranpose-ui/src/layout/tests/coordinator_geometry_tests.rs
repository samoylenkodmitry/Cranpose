use cranpose_core::NodeId;
use cranpose_ui_graphics::{DrawPrimitive, DrawScope as _, Rect};

use super::*;
use crate::{
    draw::DrawCommand,
    layout::{
        core::Alignment,
        policies::{BoxMeasurePolicy, LeafMeasurePolicy},
    },
    modifier::{Color, Modifier, Size},
    text::TextStyle,
    widgets::nodes::LayoutNode,
};

/// Composes `content` in a 100 by 100 box, measures it and hands `inspect`
/// the box's only child.
fn with_measured_child<R>(
    content: impl Fn() + Clone + 'static,
    inspect: impl FnOnce(&mut LayoutNode) -> R,
) -> R {
    let mut composition = crate::run_test_composition(move || {
        crate::widgets::Layout(
            Modifier::empty(),
            BoxMeasurePolicy::new(Alignment::TOP_START, false),
            content.clone(),
        );
    });
    let root = composition.root().expect("composition root");
    let handle = composition.runtime_handle();
    let mut applier = composition.applier_mut();
    applier.set_runtime_handle(handle);
    measure_layout(&mut applier, root, Size::new(100.0, 100.0)).expect("layout measurement");
    let child: NodeId = applier
        .with_node::<LayoutNode, _>(root, |node| node.children[0])
        .expect("root node");
    let result = applier
        .with_node::<LayoutNode, _>(child, inspect)
        .expect("child node");
    applier.clear_runtime_handle();
    result
}

/// The rects the child's draw commands fill, recorded at its measured size.
fn child_drawn_rects(modifier: Modifier, content: Size) -> Vec<Rect> {
    with_measured_child(
        move || {
            crate::widgets::Layout(modifier.clone(), LeafMeasurePolicy::new(content), || {});
        },
        |node| {
            let size = node.layout_state().size();
            node.modifier_slices_snapshot()
                .draw_commands()
                .iter()
                .flat_map(|command| {
                    let mut scope = crate::draw::command_draw_scope(size);
                    match command {
                        DrawCommand::Behind(draw)
                        | DrawCommand::WithContent(draw)
                        | DrawCommand::Overlay(draw) => draw(&mut scope),
                    }
                    scope.into_primitives()
                })
                .filter_map(|primitive| match primitive {
                    DrawPrimitive::Rect { rect, .. } | DrawPrimitive::RoundRect { rect, .. } => {
                        Some(rect)
                    }
                    _ => None,
                })
                .collect()
        },
    )
}

#[test]
fn a_background_after_a_centring_modifier_fills_the_content_it_centred() {
    let rects = child_drawn_rects(
        Modifier::empty()
            .padding(4.0)
            .minimum_interactive_component_size()
            .background(Color::WHITE),
        Size::new(10.0, 10.0),
    );
    assert_eq!(
        rects,
        [Rect {
            x: 23.0,
            y: 23.0,
            width: 10.0,
            height: 10.0
        }]
    );
}

#[test]
fn a_background_between_two_layout_modifiers_fills_the_inner_ones_rect() {
    let rects = child_drawn_rects(
        Modifier::empty()
            .padding(4.0)
            .background(Color::WHITE)
            .minimum_interactive_component_size(),
        Size::new(10.0, 10.0),
    );
    assert_eq!(
        rects,
        [Rect {
            x: 4.0,
            y: 4.0,
            width: 48.0,
            height: 48.0
        }]
    );
}

#[test]
fn a_draw_behind_an_offset_stays_while_one_after_it_moves() {
    let offset = Modifier::empty().offset(6.0, 2.0);
    let before = child_drawn_rects(
        Modifier::empty()
            .background(Color::WHITE)
            .then(offset.clone()),
        Size::new(10.0, 10.0),
    );
    assert_eq!(
        before,
        [Rect {
            x: -6.0,
            y: -2.0,
            width: 10.0,
            height: 10.0
        }]
    );
    let after = child_drawn_rects(offset.background(Color::WHITE), Size::new(10.0, 10.0));
    assert_eq!(
        after,
        [Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0
        }]
    );
}

#[test]
fn text_is_placed_where_its_centring_modifier_put_it() {
    let (node_size, text_rect) = with_measured_child(
        || {
            crate::widgets::Text(
                "Hi",
                Modifier::empty()
                    .padding(2.0)
                    .minimum_interactive_component_size(),
                TextStyle::default(),
            );
        },
        |node| {
            let size = node.layout_state().size();
            (
                size,
                node.modifier_slices_snapshot().text_content_rect(size),
            )
        },
    );
    assert_eq!(node_size, Size::new(52.0, 52.0));
    assert!(text_rect.width < 48.0 && text_rect.height < 48.0);
    assert_eq!(
        text_rect.x,
        2.0 + ((48.0_f32 - text_rect.width) / 2.0).round()
    );
    assert_eq!(
        text_rect.y,
        2.0 + ((48.0_f32 - text_rect.height) / 2.0).round()
    );
}
