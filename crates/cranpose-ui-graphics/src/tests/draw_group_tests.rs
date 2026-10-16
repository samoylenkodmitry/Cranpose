use crate::{
    Brush, Color, DrawGroup, DrawPrimitive, DrawScope, DrawScopeDefault, DrawTransform, EdgeInsets,
    GroupMarker, Point, RecordLane, Rect, Size,
};

fn red() -> Brush {
    Brush::solid(Color::RED)
}

fn starts(primitives: &[DrawPrimitive]) -> Vec<DrawGroup> {
    primitives
        .iter()
        .filter_map(|primitive| match primitive {
            DrawPrimitive::Group(GroupMarker::Start(group)) => Some(*group),
            _ => None,
        })
        .collect()
}

fn ends(primitives: &[DrawPrimitive]) -> usize {
    primitives
        .iter()
        .filter(|primitive| matches!(primitive, DrawPrimitive::Group(GroupMarker::End)))
        .count()
}

#[test]
fn a_scale_or_turn_keeps_its_pivot_in_place() {
    let mut scope = DrawScopeDefault::new(Size::new(100.0, 60.0));
    let pivot = Point::new(30.0, 20.0);
    scope.scale(0.5, 0.25, pivot, &mut |scope| scope.draw_rect(red()));
    scope.rotate(90.0, pivot, &mut |scope| scope.draw_rect(red()));
    let groups = starts(&scope.into_primitives());
    assert_eq!(groups.len(), 2);
    for group in groups {
        let moved = group.transform_to_parent().map_point(pivot);
        assert!(
            (moved.x - pivot.x).abs() < 1e-4 && (moved.y - pivot.y).abs() < 1e-4,
            "{group:?} moves its pivot to {moved:?}"
        );
    }
}

#[test]
fn steps_nest_in_order_and_an_inset_shrinks_the_scope() {
    let mut scope = DrawScopeDefault::new(Size::new(100.0, 60.0));
    let mut seen = Vec::new();
    scope.with_transform(
        &[
            DrawTransform::Translate { dx: 5.0, dy: 0.0 },
            DrawTransform::Inset {
                left: 10.0,
                top: 4.0,
                right: 20.0,
                bottom: 6.0,
            },
            DrawTransform::ClipRect(Rect {
                x: 0.0,
                y: 0.0,
                width: 8.0,
                height: 8.0,
            }),
        ],
        &mut |scope| {
            seen.push(scope.size());
            scope.draw_rect(red());
        },
    );
    assert_eq!(seen, [Size::new(70.0, 50.0)]);
    assert_eq!(
        scope.size(),
        Size::new(100.0, 60.0),
        "the scope gets its size back"
    );
    let primitives = scope.into_primitives();
    let groups = starts(&primitives);
    assert_eq!(groups.len(), 3);
    assert_eq!(ends(&primitives), 3);
    assert_eq!(groups[0].translation, Point::new(5.0, 0.0));
    assert_eq!(groups[1].translation, Point::new(10.0, 4.0));
    assert!(groups[2].clip);
    assert!(matches!(
        primitives[3],
        DrawPrimitive::Rect { rect, .. } if rect == Rect::from_size(Size::new(70.0, 50.0))
    ));
}

#[test]
fn a_path_clip_ends_with_its_mask_inside_an_isolated_group() {
    let mut path = crate::Path::new();
    path.move_to(Point::new(10.0, 10.0));
    path.line_to(Point::new(30.0, 10.0));
    path.line_to(Point::new(20.0, 30.0));
    path.close();
    let mut scope = DrawScopeDefault::new(Size::new(40.0, 40.0));
    scope.clip_path(&path, &mut |scope| scope.draw_rect(red()));
    let primitives = scope.into_primitives();
    let [group] = starts(&primitives)[..] else {
        panic!("one group");
    };
    assert!(group.isolated && group.clip);
    let mask = &primitives[primitives.len() - 2];
    assert!(
        matches!(
            mask,
            DrawPrimitive::Blend { primitive, blend_mode: crate::BlendMode::DstOut }
                if matches!(**primitive, DrawPrimitive::Image { rect, .. } if rect == group.bounds)
        ),
        "the mask covers the group's bounds, got {mask:?}"
    );
}

#[test]
fn content_drawn_inside_a_group_leaves_both_halves_balanced() {
    let mut scope = DrawScopeDefault::new(Size::new(40.0, 40.0));
    scope.translate(3.0, 0.0, &mut |scope| {
        scope.scale(2.0, 2.0, Point::ZERO, &mut |scope| {
            scope.draw_rect(red());
            scope.draw_content();
            scope.draw_rect(red());
        });
    });
    let recording = scope.finish();
    let balance = |segments: std::ops::Range<u32>| {
        recording
            .segments_in(&segments)
            .fold((0i32, true), |(depth, never_below), segment| {
                let depth = depth
                    + match segment.lane {
                        RecordLane::GroupStart => 1,
                        RecordLane::GroupEnd => -1,
                        _ => 0,
                    };
                (depth, never_below && depth >= 0)
            })
    };
    assert_eq!(balance(recording.content_split(true)), (0, true));
    assert_eq!(balance(recording.content_split(false)), (0, true));
    let behind: Vec<_> = recording
        .primitives(recording.content_split(true))
        .collect();
    let overlay: Vec<_> = recording
        .primitives(recording.content_split(false))
        .collect();
    assert_eq!(
        starts(&behind),
        starts(&overlay),
        "the overlay reopens the same groups"
    );
}

#[test]
fn a_padded_draw_moves_its_groups_and_not_what_they_hold() {
    let mut scope = DrawScopeDefault::new(Size::new(40.0, 40.0));
    scope.draw_inset(EdgeInsets::from_components(4.0, 6.0, 0.0, 0.0), |inner| {
        inner.draw_rect_at(Rect::from_size(Size::new(2.0, 2.0)), red());
        inner.translate(10.0, 0.0, &mut |scope| {
            scope.draw_rect_at(Rect::from_size(Size::new(2.0, 2.0)), red());
        });
    });
    let primitives = scope.into_primitives();
    assert!(matches!(
        primitives[0],
        DrawPrimitive::Rect { rect, .. } if (rect.x, rect.y) == (4.0, 6.0)
    ));
    assert_eq!(starts(&primitives)[0].translation, Point::new(14.0, 6.0));
    assert!(matches!(
        primitives[2],
        DrawPrimitive::Rect { rect, .. } if (rect.x, rect.y) == (0.0, 0.0)
    ));
}
