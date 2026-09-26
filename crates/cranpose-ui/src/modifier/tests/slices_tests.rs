use std::cell::RefCell;

use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, Rect, Size};

use super::*;

#[test]
fn local_pointer_dispatch_preserves_consumption_and_terminal_delivery() {
    let delivered = Rc::new(RefCell::new(Vec::new()));
    let first = Rc::clone(&delivered);
    let second = Rc::clone(&delivered);
    let slices = ModifierNodeSlices {
        pointer_inputs: vec![
            Rc::new(move |event| {
                first.borrow_mut().push((1, event.kind, event.position));
                event.consume();
            }),
            Rc::new(move |event| second.borrow_mut().push((2, event.kind, event.position))),
        ],
        ..Default::default()
    };
    let local = Point { x: 12.0, y: 8.0 };
    for kind in [
        PointerEventKind::Down,
        PointerEventKind::Up,
        PointerEventKind::Cancel,
    ] {
        slices.dispatch_pointer_event(PointerEvent::new(kind, local, local));
    }
    assert_eq!(
        *delivered.borrow(),
        [
            (1, PointerEventKind::Down, local),
            (1, PointerEventKind::Up, local),
            (2, PointerEventKind::Up, local),
            (1, PointerEventKind::Cancel, local),
            (2, PointerEventKind::Cancel, local),
        ]
    );
}

#[test]
fn local_pointer_dispatch_calls_click_handlers_only_for_an_unconsumed_press() {
    let clicks = Rc::new(RefCell::new(Vec::new()));
    let recorded = Rc::clone(&clicks);
    let slices = ModifierNodeSlices {
        click_handlers: vec![Rc::new(move |position| {
            recorded.borrow_mut().push(position);
        })],
        ..Default::default()
    };
    let local = Point { x: 24.0, y: 16.0 };
    slices.dispatch_pointer_event(PointerEvent::new(PointerEventKind::Down, local, local));
    slices.dispatch_pointer_event(PointerEvent::new(PointerEventKind::Up, local, local));
    let consumed = PointerEvent::new(PointerEventKind::Down, local, local);
    consumed.consume();
    slices.dispatch_pointer_event(consumed);
    assert_eq!(*clicks.borrow(), [local]);
}

fn recorded(command: &DrawCommand, size: Size) -> Vec<DrawPrimitive> {
    use cranpose_ui_graphics::DrawScope as _;
    let mut scope = crate::draw::command_draw_scope(size);
    match command {
        DrawCommand::Behind(draw) | DrawCommand::WithContent(draw) | DrawCommand::Overlay(draw) => {
            draw(&mut scope);
        }
    }
    scope.into_primitives()
}

fn drawn_rects(modifier: Modifier) -> Vec<Rect> {
    let size = Size {
        width: 20.0,
        height: 10.0,
    };
    collect_slices_from_modifier(&modifier)
        .draw_commands()
        .iter()
        .flat_map(|command| recorded(command, size))
        .filter_map(|primitive| match primitive {
            DrawPrimitive::Rect { rect, .. } | DrawPrimitive::RoundRect { rect, .. } => Some(rect),
            _ => None,
        })
        .collect()
}

#[test]
fn a_background_after_padding_fills_only_the_padded_rect() {
    let rects = drawn_rects(
        Modifier::empty()
            .padding(1.0)
            .background(Color::WHITE)
            .rounded_corners(3.0),
    );
    assert_eq!(
        rects,
        [Rect {
            x: 1.0,
            y: 1.0,
            width: 18.0,
            height: 8.0,
        }]
    );
}

#[test]
fn a_background_before_padding_fills_the_node() {
    let rects = drawn_rects(Modifier::empty().background(Color::WHITE).padding(1.0));
    assert_eq!(
        rects,
        [Rect {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 10.0,
        }]
    );
}

#[test]
fn a_draw_after_padding_sees_and_fills_the_padded_rect() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sizes = Rc::clone(&seen);
    let rects = drawn_rects(
        Modifier::empty()
            .padding_symmetric(2.0, 1.0)
            .padding(1.0)
            .draw_behind(move |scope| {
                sizes.borrow_mut().push(scope.size());
                scope.draw_rect(Brush::solid(Color::WHITE));
            }),
    );
    assert_eq!(
        *seen.borrow(),
        [Size {
            width: 14.0,
            height: 6.0,
        }]
    );
    assert_eq!(
        rects,
        [Rect {
            x: 3.0,
            y: 2.0,
            width: 14.0,
            height: 6.0,
        }]
    );
}

#[test]
fn a_draw_with_content_after_padding_keeps_its_content_marker() {
    let modifier = Modifier::empty().padding(2.0).draw_with_content(|scope| {
        scope.draw_content();
        scope.draw_rect(Brush::solid(Color::WHITE));
    });
    let slices = collect_slices_from_modifier(&modifier);
    let primitives = recorded(
        &slices.draw_commands()[0],
        Size {
            width: 20.0,
            height: 10.0,
        },
    );
    assert!(matches!(primitives[0], DrawPrimitive::Content));
    assert!(matches!(
        primitives[1],
        DrawPrimitive::Rect {
            rect: Rect {
                x: 2.0,
                y: 2.0,
                width: 16.0,
                height: 6.0,
            },
            ..
        }
    ));
}

#[test]
fn a_tilt_merged_with_a_surface_keeps_its_camera_and_pivot() {
    let tilt = GraphicsLayer {
        rotation_y: 12.0,
        camera_distance: 12.0,
        transform_origin: cranpose_ui_graphics::TransformOrigin::new(0.0, 1.0),
        ..Default::default()
    };
    let surface = GraphicsLayer {
        clip: true,
        ..Default::default()
    };

    let merged = merge_graphics_layers(tilt.clone(), surface);
    assert_eq!(merged.camera_distance, 12.0);
    assert_eq!(merged.transform_origin, tilt.transform_origin);

    let turn = GraphicsLayer {
        rotation_z: 30.0,
        camera_distance: 20.0,
        ..Default::default()
    };
    let over_a_turn = merge_graphics_layers(tilt, turn);
    assert_eq!(
        over_a_turn.camera_distance, 20.0,
        "a later layer that turns frames the merged transform"
    );
}
