// The TV tab driven the way a remote drives it, through the shell's key
// entry: the D-pad reaches a card, its center opens the player, Right seeks
// before focus could move, and Back closes the player.

use cranpose_testing::robot::{RobotTestRule, TestRenderer};
use cranpose_ui::{Key, KeyEvent};
use desktop_app::app::{combined_app_with_initial_tab, DemoTab};

fn press(robot: &mut RobotTestRule<TestRenderer>, key: Key) -> bool {
    let taken = robot.shell_mut().on_key_event(&KeyEvent::key_down(key, ""));
    robot.wait_for_idle();
    taken
}

fn shows(robot: &mut RobotTestRule<TestRenderer>, text: &str) -> bool {
    robot
        .get_all_text()
        .iter()
        .any(|shown| shown.contains(text))
}

#[test]
fn a_remote_opens_a_card_seeks_and_closes_the_player() {
    let mut robot = RobotTestRule::new(1280, 720, TestRenderer::default(), || {
        combined_app_with_initial_tab(Some(DemoTab::Tv));
    });
    robot.wait_for_idle();
    // From nothing focused: the source toggle, the tab bar, the first card.
    for _ in 0..3 {
        assert!(press(&mut robot, Key::DirectionDown));
    }
    assert!(
        press(&mut robot, Key::DirectionCenter),
        "the center opens the card"
    );
    assert!(shows(&mut robot, "Row 1, title 1"));
    assert!(shows(&mut robot, "0:00 of 60:00"));

    assert!(press(&mut robot, Key::DirectionRight), "the player seeks");
    assert!(shows(&mut robot, "0:10 of 60:00"));

    assert!(press(&mut robot, Key::Back), "Back closes the player");
    assert!(!shows(&mut robot, "of 60:00"));
    assert!(shows(&mut robot, "Row 1"));
}
