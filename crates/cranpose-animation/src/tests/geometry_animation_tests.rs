use super::*;
use crate::{
    animation::AnimationSpec,
    test_support::{
        assert_interpolates_to_target, assert_retargets_mid_flight_without_snapping,
        assert_spring_scalar_round_trips,
    },
};

#[test]
fn point_size_and_rect_satisfy_the_spring_scalar_contract() {
    assert_spring_scalar_round_trips(Point::ZERO, Point::new(10.0, -20.0));
    assert_spring_scalar_round_trips(Size::ZERO, Size::new(30.0, 40.0));
    assert_spring_scalar_round_trips(
        Rect::from_size(Size::ZERO),
        Rect {
            x: 5.0,
            y: -5.0,
            width: 50.0,
            height: 60.0,
        },
    );
}

#[test]
fn animate_offset_as_state_interpolates_from_the_previous_value_to_the_target() {
    assert_interpolates_to_target(Point::ZERO, Point::new(100.0, -40.0), |target| {
        animateOffsetAsState(
            target,
            AnimationType::Tween(AnimationSpec::linear(160)),
            "offset",
        )
    });
}

#[test]
fn animate_size_as_state_interpolates_from_the_previous_value_to_the_target() {
    assert_interpolates_to_target(Size::ZERO, Size::new(200.0, 80.0), |target| {
        animateSizeAsState(
            target,
            AnimationType::Tween(AnimationSpec::linear(160)),
            "size",
        )
    });
}

#[test]
fn animate_rect_as_state_interpolates_from_the_previous_value_to_the_target() {
    let start = Rect::from_size(Size::ZERO);
    let end = Rect {
        x: 10.0,
        y: 20.0,
        width: 300.0,
        height: 150.0,
    };
    assert_interpolates_to_target(start, end, |target| {
        animateRectAsState(
            target,
            AnimationType::Tween(AnimationSpec::linear(160)),
            "rect",
        )
    });
}

#[test]
fn animate_offset_as_state_retargets_mid_flight_instead_of_restarting() {
    assert_retargets_mid_flight_without_snapping(
        Point::ZERO,
        Point::new(100.0, 100.0),
        6,
        Point::new(-30.0, 10.0),
        |target| {
            animateOffsetAsState(
                target,
                AnimationType::Tween(AnimationSpec::linear(400)),
                "offset",
            )
        },
    );
}

#[test]
fn animate_rect_as_state_retargets_mid_flight_instead_of_restarting() {
    let start = Rect::from_size(Size::ZERO);
    let first_target = Rect {
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 200.0,
    };
    let second_target = Rect {
        x: 40.0,
        y: 40.0,
        width: 20.0,
        height: 20.0,
    };
    assert_retargets_mid_flight_without_snapping(start, first_target, 6, second_target, |target| {
        animateRectAsState(
            target,
            AnimationType::Tween(AnimationSpec::linear(400)),
            "rect",
        )
    });
}
