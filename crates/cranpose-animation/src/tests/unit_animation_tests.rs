use super::*;
use crate::{
    animation::AnimationSpec,
    test_support::{
        assert_interpolates_to_target, assert_retargets_mid_flight_without_snapping,
        assert_spring_scalar_round_trips,
    },
};

#[test]
fn dp_and_sp_satisfy_the_spring_scalar_contract() {
    assert_spring_scalar_round_trips(Dp(0.0), Dp(100.0));
    assert_spring_scalar_round_trips(Sp(10.0), Sp(24.0));
}

#[test]
fn animate_dp_as_state_interpolates_from_the_previous_value_to_the_target() {
    assert_interpolates_to_target(Dp(0.0), Dp(100.0), |target| {
        animateDpAsState(
            target,
            AnimationType::Tween(AnimationSpec::linear(160)),
            "dp",
        )
    });
}

#[test]
fn animate_dp_as_state_retargets_mid_flight_instead_of_restarting() {
    assert_retargets_mid_flight_without_snapping(Dp(0.0), Dp(100.0), 6, Dp(-60.0), |target| {
        animateDpAsState(
            target,
            AnimationType::Tween(AnimationSpec::linear(400)),
            "dp",
        )
    });
}
