//! Typing into a plain and a secure text field: characters beyond ASCII
//! (accents, CJK, an emoji) reach the field as they were typed, the secure
//! field keeps what was typed out of what readers see, and Backspace takes
//! one character back, an emoji included.

use cranpose::Robot;
use cranpose_core::remember;
use cranpose_foundation::text::TextFieldState;
use cranpose_testing::{find_in_semantics, find_text_exact};
use cranpose_ui::{
    BasicSecureTextField, BasicTextField, Column, ColumnSpec, Modifier, SecureTextFieldOptions,
    Text, TextStyle,
};

use crate::{robot_exit, robot_launch, text_input_robot_helpers};

const PLAIN: &str = "h\u{e9}llo \u{65e5}\u{672c} \u{1f44b}";
const SECRET: &str = "p\u{e4}ssw\u{f6}rd";

fn wait_for(robot: &Robot, text: &str, step: &str) {
    let found = text_input_robot_helpers::wait_for_in_semantics(robot, |robot| {
        find_in_semantics(robot, |element| find_text_exact(element, text))
    });
    if found.is_none() {
        robot_exit::fail(robot, &format!("{step}: {text:?} never showed"));
    }
}

fn focus(robot: &Robot, tag: &str) {
    if let Err(error) = robot.click_by_tag(tag) {
        robot_exit::fail(robot, &format!("focusing {tag:?} failed: {error}"));
    }
}

fn send(robot: &Robot, sent: Result<(), String>, what: &str) {
    if let Err(error) = sent {
        robot_exit::fail(robot, &format!("{what} failed: {error}"));
    }
}

pub(crate) fn main() {
    env_logger::init();
    robot_launch::launch("Secure and Unicode Input", 480, 320)
        .with_test_driver(|robot| {
            robot_exit::arm_timeout(60);
            wait_for(&robot, "plain 0, secret 0", "first frame");

            focus(&robot, "plain");
            send(&robot, robot.type_text(PLAIN), "typing the plain text");
            wait_for(&robot, &format!("plain {}, secret 0", PLAIN.chars().count()), "plain typed");
            wait_for(&robot, PLAIN, "the plain field shows what was typed");
            println!("PASS: accents, CJK and an emoji reach a plain field as typed");

            send(&robot, robot.send_key("BackSpace"), "Backspace in the plain field");
            wait_for(
                &robot,
                &format!("plain {}, secret 0", PLAIN.chars().count() - 1),
                "Backspace took the emoji",
            );
            println!("PASS: Backspace takes one character back, an emoji included");

            focus(&robot, "secret");
            send(&robot, robot.type_text(SECRET), "typing the secret");
            let typed = SECRET.chars().count();
            wait_for(
                &robot,
                &format!("plain {}, secret {typed}", PLAIN.chars().count() - 1),
                "secret typed",
            );
            if find_in_semantics(&robot, |element| find_text_exact(element, SECRET)).is_some() {
                robot_exit::fail(&robot, "the secure field showed its text to readers");
            }
            println!("PASS: a secure field takes the text and keeps it from readers");
            let _ = robot.exit();
        })
        .run(|| {
            let plain = remember(|| TextFieldState::new("")).with(|state| *state);
            let secret = remember(|| TextFieldState::new("")).with(|state| *state);
            Column(Modifier::empty().padding(16.0), ColumnSpec::default(), move || {
                Text(
                    format!(
                        "plain {}, secret {}",
                        plain.text().chars().count(),
                        secret.text().chars().count()
                    ),
                    Modifier::empty(),
                    TextStyle::default(),
                );
                BasicTextField(
                    plain,
                    Modifier::empty().width(400.0).test_tag("plain"),
                    TextStyle::default(),
                );
                BasicSecureTextField(
                    secret,
                    Modifier::empty().width(400.0).test_tag("secret"),
                    SecureTextFieldOptions::default(),
                );
            });
        });
}
