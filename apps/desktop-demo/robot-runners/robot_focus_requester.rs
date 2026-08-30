mod robot_launch;

mod robot_exit;

mod text_input_robot_helpers;

use std::time::Duration;

use cranpose_testing::find_button_in_semantics;
use desktop_app::app;

/// Proves `FocusRequester` end to end on a live window: clicking "Focus Field
/// 2" never touches the text field itself, only the button next to it, yet
/// the keystrokes typed right after land in that field. If the field never
/// received real focus, `text_field_focus` would have nothing to dispatch the
/// keys to and this would fail.
fn main() {
    env_logger::init();
    println!("=== Robot Focus Requester Test ===");
    println!("Testing Modifier::focus_requester() moving keyboard focus without a tap\n");

    const TEST_TIMEOUT_SECS: u64 = 60;

    robot_launch::launch("Robot Focus Requester Test", 900, 700)
        .with_test_driver(|robot| {
            robot_exit::arm_timeout(TEST_TIMEOUT_SECS);

            std::thread::sleep(Duration::from_millis(300));
            println!("✓ App launched\n");

            let mut all_passed = true;

            println!("--- Step 1: Open Text Input tab ---");
            if text_input_robot_helpers::open_text_input_tab(&robot) {
                println!("✓ Text Input tab visible\n");
            } else {
                println!("✗ FAIL: Could not open the Text Input tab");
                let _ = robot.exit();
                return;
            }

            println!("--- Step 2: Field 2 starts empty and unfocused ---");
            match text_input_robot_helpers::wait_for_in_semantics(&robot, |robot| {
                cranpose_testing::find_text_in_semantics(robot, "Field 2 value: \"\"")
            }) {
                Some(_) => println!("✓ PASS: Field 2 starts empty\n"),
                None => {
                    println!("✗ FAIL: Could not find the empty 'Field 2 value' readout\n");
                    all_passed = false;
                }
            }

            println!("--- Step 3: Click 'Focus Field 2' (never touches the field) ---");
            match text_input_robot_helpers::wait_for_in_semantics(&robot, |robot| {
                find_button_in_semantics(robot, "Focus Field 2")
            }) {
                Some(bounds) => {
                    if text_input_robot_helpers::click_bounds(&robot, bounds).is_ok() {
                        println!("✓ Clicked 'Focus Field 2'\n");
                    } else {
                        println!("✗ FAIL: Could not click 'Focus Field 2'\n");
                        all_passed = false;
                    }
                }
                None => {
                    println!("✗ FAIL: Could not find the 'Focus Field 2' button\n");
                    all_passed = false;
                }
            }

            println!("--- Step 4: Type without ever tapping the field ---");
            for ch in ["h", "i"] {
                let _ = robot.send_key(ch);
                std::thread::sleep(Duration::from_millis(30));
            }
            let _ = robot.wait_for_idle();
            std::thread::sleep(Duration::from_millis(200));

            match text_input_robot_helpers::wait_for_in_semantics(&robot, |robot| {
                cranpose_testing::find_text_in_semantics(robot, "Field 2 value: \"hi\"")
            }) {
                Some(_) => println!("✓ PASS: Field 2 received the typed keys via FocusRequester\n"),
                None => {
                    println!(
                        "✗ FAIL: Field 2 did not pick up the typed keys — FocusRequester did not \
                     move real keyboard focus\n"
                    );
                    all_passed = false;
                }
            }

            println!(
                "=== Result: {} ===",
                if all_passed { "PASS" } else { "FAIL" }
            );
            let _ = robot.exit();

            if !all_passed {
                std::process::exit(1);
            }
        })
        .run(|| {
            app::combined_app();
        });
}
