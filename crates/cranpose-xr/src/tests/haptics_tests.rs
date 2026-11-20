use super::*;

fn buzz(amplitude: f32) -> Vibration {
    Vibration::new(amplitude, Duration::from_millis(40))
}

#[test]
fn the_newest_ask_of_a_hand_replaces_the_older_one_and_the_hands_stay_apart() {
    let haptics = Haptics::new();
    haptics.vibrate(Hand::Left, buzz(0.2));
    haptics.vibrate(Hand::Left, buzz(0.9));
    haptics.vibrate(Hand::Right, buzz(0.5));
    haptics.stop(Hand::Right);
    let [left, right] = haptics.take();
    assert_eq!(left, Some(HapticCommand::Vibrate(buzz(0.9))));
    assert_eq!(right, Some(HapticCommand::Stop));
}

#[test]
fn an_ask_reaches_the_frame_loop_once() {
    let haptics = Haptics::new();
    haptics.vibrate(Hand::Right, buzz(1.0));
    assert_eq!(haptics.take()[1], Some(HapticCommand::Vibrate(buzz(1.0))));
    assert_eq!(haptics.take(), [None, None]);
}

#[test]
fn a_clone_on_another_thread_asks_for_the_same_motors() {
    let haptics = Haptics::new();
    let remote = haptics.clone();
    std::thread::spawn(move || remote.vibrate(Hand::Left, buzz(0.5)))
        .join()
        .expect("the thread ends");
    assert_eq!(haptics.take()[0], Some(HapticCommand::Vibrate(buzz(0.5))));
}

#[test]
fn a_vibration_keeps_to_what_a_controller_takes() {
    assert_eq!(buzz(3.0).amplitude(), 1.0);
    assert_eq!(buzz(-1.0).amplitude(), 0.0);
    assert_eq!(buzz(f32::NAN).amplitude(), 0.0);
    assert_eq!(buzz(0.5).frequency(), None);
    assert_eq!(buzz(0.5).with_frequency(160.0).frequency(), Some(160.0));
    assert_eq!(buzz(0.5).with_frequency(-1.0).frequency(), None);
    assert_eq!(buzz(0.5).with_frequency(f32::NAN).frequency(), None);
}

#[test]
fn a_side_that_a_tracked_hand_points_for_takes_no_vibration() {
    let asks = [
        Some(HapticCommand::Vibrate(buzz(0.5))),
        Some(HapticCommand::Stop),
    ];
    assert_eq!(controller_asks(asks, [None, None]), asks);
    let controller = Some(InputSource::Controller);
    let hands = Some(InputSource::Hands);
    assert_eq!(
        controller_asks(asks, [controller, hands]),
        [Some(HapticCommand::Vibrate(buzz(0.5))), None]
    );
    assert_eq!(
        controller_asks(asks, [hands, controller]),
        [None, Some(HapticCommand::Stop)]
    );
    assert_eq!(controller_asks(asks, [hands, hands]), [None, None]);
}
