//! The launcher's process start runs once, before the first composition.

use std::sync::atomic::{AtomicU32, Ordering};

use cranpose_ui::{composable, Modifier, Text, TextStyle};

use crate::{robot_exit, robot_launch};

static STARTS: AtomicU32 = AtomicU32::new(0);

fn count_start() {
    STARTS.fetch_add(1, Ordering::SeqCst);
}

pub(crate) fn main() {
    robot_launch::launch("Process Start", 480, 240)
        .with_process_start(count_start)
        .with_test_driver(|robot| {
            robot_exit::arm_timeout(30);
            robot.wait_for_idle().expect("initial frame");
            if robot.validate_content("Started 1 time").is_err() {
                robot_exit::fail(&robot, "the process start did not run once before composing");
            }
            robot.exit().expect("exit process start robot");
        })
        .run(process_start_probe);
}

#[composable]
fn process_start_probe() {
    let starts = STARTS.load(Ordering::SeqCst);
    let label = format!("Started {starts} time{}", if starts == 1 { "" } else { "s" });
    Text(label, Modifier::empty(), TextStyle::default());
}
