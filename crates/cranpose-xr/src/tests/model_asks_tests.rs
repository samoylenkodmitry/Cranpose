// When the backend asks the runtime for the model of each controller.

use super::*;

const TOUCH: Option<InteractionProfile> = Some(InteractionProfile::MetaTouchPlus);
const HANDS: Option<InteractionProfile> = Some(InteractionProfile::ExtHand);

// The runtime's model.
fn model() -> Offer {
    Offer::Bytes(std::sync::Arc::from(&b"glTF"[..]))
}

#[test]
fn both_hands_are_due_until_the_runtime_answers_whether_or_not_a_controller_is_held() {
    let mut asks = ModelAsks::new();
    assert_eq!(asks.due([None, None]), [true, true]);
    assert_eq!(asks.due([HANDS, TOUCH]), [true, true]);
}

#[test]
fn a_hand_that_got_its_model_is_never_due_again() {
    let mut asks = ModelAsks::new();
    asks.due([None, None]);
    asks.answered(Hand::Left, &model());
    asks.answered(Hand::Right, &model());
    for profiles in [[None, None], [TOUCH, TOUCH], [HANDS, TOUCH], [TOUCH, None]] {
        assert_eq!(asks.due(profiles), [false, false], "{profiles:?}");
    }
}

#[test]
fn a_hand_with_no_model_asks_again_when_a_controller_comes_into_it() {
    // Android XR and an app that did not declare the models: the runtime has none, so the first
    // answer is no
    let mut asks = ModelAsks::new();
    asks.due([None, None]);
    asks.answered(Hand::Left, &Offer::Absent);
    asks.answered(Hand::Right, &Offer::Absent);
    assert_eq!(asks.due([None, None]), [false, false]);
    // The left controller comes into the hand
    assert_eq!(asks.due([TOUCH, None]), [true, false]);
    asks.answered(Hand::Left, &Offer::Absent);
    // It stays: no more questions
    assert_eq!(asks.due([TOUCH, None]), [false, false]);
    // It leaves and returns
    assert_eq!(asks.due([None, None]), [false, false]);
    assert_eq!(asks.due([TOUCH, None]), [true, false]);
}

#[test]
fn a_tracked_hand_is_not_a_controller_that_comes_back() {
    let mut asks = ModelAsks::new();
    asks.due([None, None]);
    asks.answered(Hand::Left, &Offer::Absent);
    asks.answered(Hand::Right, &Offer::Absent);
    assert_eq!(asks.due([HANDS, HANDS]), [false, false]);
    assert_eq!(asks.due([None, HANDS]), [false, false]);
}

#[test]
fn each_hand_keeps_its_own_schedule() {
    let mut asks = ModelAsks::new();
    asks.due([TOUCH, TOUCH]);
    asks.answered(Hand::Left, &model());
    asks.answered(Hand::Right, &Offer::Absent);
    assert_eq!(asks.due([None, None]), [false, false]);
    assert_eq!(asks.due([TOUCH, TOUCH]), [false, true]);
}
