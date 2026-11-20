use super::*;

fn inputs(profile: InteractionProfile, hand: Hand) -> Vec<Binding> {
    bindings(profile, hand)
        .iter()
        .flat_map(|list| list.iter().copied())
        .collect()
}

fn paths(profile: InteractionProfile, hand: Hand) -> Vec<&'static str> {
    inputs(profile, hand)
        .into_iter()
        .map(|(_, path)| path)
        .collect()
}

#[test]
fn every_profile_keeps_the_pointer_of_a_hand() {
    for profile in InteractionProfile::ALL {
        for hand in Hand::BOTH {
            let bound = inputs(profile, hand);
            for pointer in [AimPose, Trigger] {
                assert!(
                    bound.iter().any(|&(input, _)| input == pointer),
                    "{profile:?} {hand:?} lacks {pointer:?}"
                );
            }
        }
    }
}

#[test]
fn a_hand_binds_each_input_and_each_path_once() {
    for profile in InteractionProfile::ALL {
        for hand in Hand::BOTH {
            let bound = inputs(profile, hand);
            for (index, (input, path)) in bound.iter().enumerate() {
                assert!(
                    bound[index + 1..]
                        .iter()
                        .all(|(other, other_path)| other != input && other_path != path),
                    "{profile:?} {hand:?} binds {input:?} or {path} twice"
                );
            }
        }
    }
}

#[test]
fn the_touch_controllers_give_each_hand_only_its_own_buttons() {
    for profile in [
        InteractionProfile::OculusTouch,
        InteractionProfile::MetaTouchPlus,
        InteractionProfile::MetaTouchPro,
    ] {
        let left = paths(profile, Hand::Left);
        let right = paths(profile, Hand::Right);
        for button in ["input/x/click", "input/y/touch", "input/menu/click"] {
            assert!(
                left.contains(&button) && !right.contains(&button),
                "{button}"
            );
        }
        for button in ["input/a/click", "input/b/touch"] {
            assert!(
                right.contains(&button) && !left.contains(&button),
                "{button}"
            );
        }
    }
}

#[test]
fn a_profile_without_a_stick_or_a_squeeze_leaves_them_unbound() {
    let hands = paths(InteractionProfile::MicrosoftHand, Hand::Left);
    assert!(hands.contains(&"input/squeeze/value"));
    assert!(
        !inputs(InteractionProfile::MicrosoftHand, Hand::Left)
            .iter()
            .any(|&(input, _)| input == StickY)
    );
    assert!(
        !inputs(InteractionProfile::MagicLeap2, Hand::Right)
            .iter()
            .any(|&(input, _)| input == Squeeze)
    );
}

#[test]
fn a_trackpad_leans_the_pointer_forward_where_a_controller_has_no_stick() {
    let vive = inputs(InteractionProfile::HtcVive, Hand::Left);
    assert!(vive.contains(&(StickY, "input/trackpad/y")));
}

#[test]
fn the_pointer_inputs_are_the_aim_the_trigger_and_the_two_stick_axes() {
    let pointing: Vec<_> = inputs(InteractionProfile::OculusTouch, Hand::Left)
        .into_iter()
        .filter(|&(input, _)| input.points())
        .map(|(_, path)| path)
        .collect();
    assert_eq!(
        pointing,
        [
            "input/aim/pose",
            "input/trigger/value",
            "input/thumbstick/x",
            "input/thumbstick/y"
        ]
    );
}

#[test]
fn the_profile_paths_are_distinct_interaction_profiles() {
    let mut all: Vec<_> = InteractionProfile::ALL.map(InteractionProfile::path).into();
    assert!(
        all.iter()
            .all(|path| path.starts_with("/interaction_profiles/"))
    );
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), InteractionProfile::ALL.len());
}
