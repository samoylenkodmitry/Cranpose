use cranpose_ui_graphics::{DrawScope, DrawScopeDefault, Rect};

use super::*;

fn recorded_command(record: impl Fn(&mut dyn DrawScope) + 'static) -> DrawCommandFn {
    Rc::new(move |scope: &mut DrawScopeDefault| record(scope))
}

fn rect_at(x: f32) -> Rect {
    Rect {
        x,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    }
}

fn rect_xs(primitives: &[DrawPrimitive]) -> Vec<f32> {
    primitives
        .iter()
        .map(|primitive| match primitive {
            DrawPrimitive::Rect { rect, .. } => rect.x,
            other => panic!("unexpected primitive {other:?}"),
        })
        .collect()
}

#[test]
fn marker_free_recording_passes_through() {
    let command = DrawCommand::Behind(recorded_command(|scope| {
        scope.draw_rect_at(rect_at(1.0), Brush::solid(Color::WHITE));
        scope.draw_rect_at(rect_at(2.0), Brush::solid(Color::WHITE));
    }));
    let out = primitives_for_placement(&command, DrawPlacement::Behind, Size::new(10.0, 10.0));
    assert_eq!(rect_xs(&out), [1.0, 2.0]);
}

#[test]
fn unmatched_placement_leaves_recording_storage_untouched() {
    let callback = recorded_command(|_| panic!("unmatched callback"));
    for (command, placement) in [
        (
            DrawCommand::Behind(callback.clone()),
            DrawPlacement::Overlay,
        ),
        (DrawCommand::Overlay(callback), DrawPlacement::Behind),
    ] {
        let mut storage = Some(CommandRecording::from_primitives([DrawPrimitive::Content]));
        let result =
            recording_for_placement_reusing(&command, placement, Size::new(10.0, 10.0), || {
                storage.take().expect("recording storage")
            });
        assert!(result.is_none());
        assert_eq!(
            storage
                .expect("unmatched placement keeps storage")
                .content_markers(),
            1
        );
    }
}

#[test]
fn recorded_markers_still_split_content_placements() {
    let with_content = recorded_command(|scope| {
        scope.draw_rect_at(rect_at(1.0), Brush::solid(Color::WHITE));
        scope.draw_content();
        scope.draw_rect_at(rect_at(2.0), Brush::solid(Color::WHITE));
    });
    let command = DrawCommand::WithContent(with_content);
    let size = Size::new(10.0, 10.0);
    let behind = primitives_for_placement(&command, DrawPlacement::Behind, size);
    assert_eq!(rect_xs(&behind), [1.0]);
    let overlay = primitives_for_placement(&command, DrawPlacement::Overlay, size);
    assert_eq!(rect_xs(&overlay), [2.0]);
}

#[test]
fn reused_storage_records_identically_to_fresh() {
    let command = DrawCommand::WithContent(recorded_command(|scope| {
        scope.draw_rect_at(rect_at(1.0), Brush::solid(Color::WHITE));
        scope.draw_content();
        scope.draw_rect_at(rect_at(2.0), Brush::solid(Color::WHITE));
    }));
    let size = Size::new(10.0, 10.0);
    for placement in [DrawPlacement::Behind, DrawPlacement::Overlay] {
        let fresh = primitives_for_placement(&command, placement, size);
        let dirty = CommandRecording::from_primitives(vec![DrawPrimitive::Content; 8]);
        let (recording, segments) =
            recording_for_placement_reusing(&command, placement, size, || dirty)
                .expect("a with-content command records for both placements");
        let reused: Vec<DrawPrimitive> = recording.primitives(segments).collect();
        assert_eq!(fresh, reused);
    }
}

#[test]
fn pushed_batches_keep_marker_count_authoritative() {
    let command = DrawCommand::Behind(Rc::new(|scope: &mut DrawScopeDefault| {
        scope.push_recorded(vec![
            DrawPrimitive::Rect {
                rect: rect_at(1.0),
                brush: Brush::solid(Color::WHITE),
                stroke: None,
            },
            DrawPrimitive::Content,
            DrawPrimitive::Rect {
                rect: rect_at(2.0),
                brush: Brush::solid(Color::WHITE),
                stroke: None,
            },
        ]);
    }));
    let out = primitives_for_placement(&command, DrawPlacement::Behind, Size::new(10.0, 10.0));
    assert_eq!(rect_xs(&out), [1.0, 2.0]);
}
