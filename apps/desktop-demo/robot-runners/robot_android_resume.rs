use crate::robot_launch;

use std::cell::RefCell;

use cranpose_core::{rememberMutableStateOf, MutableState};
use cranpose_ui::{composable, Column, ColumnSpec, LinearArrangement, Modifier, Text, TextStyle};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResumeState {
    Attached,
    Background,
    Resumed,
}

thread_local! {
    static RESUME_STATE: RefCell<Option<MutableState<ResumeState>>> = const { RefCell::new(None) };
}

fn lifecycle_hook(name: String, argument: String) -> Result<Option<String>, String> {
    if !argument.is_empty() {
        return Err(format!("unexpected lifecycle argument {argument:?}"));
    }
    let state = RESUME_STATE
        .with(|slot| (*slot.borrow()).ok_or_else(|| "resume state unavailable".to_string()))?;
    match name.as_str() {
        "terminate-window" => state.set(ResumeState::Background),
        "init-window" => state.set(ResumeState::Resumed),
        _ => return Err(format!("unknown lifecycle event {name:?}")),
    }
    Ok(None)
}

pub(crate) fn main() {
    robot_launch::launch("Android Resume", 640, 480)
        .with_robot_app_hook(lifecycle_hook)
        .with_test_driver(|robot| {
            robot.wait_for_idle().expect("initial frame");
            robot.validate_content("Attached").expect("initial content");

            robot
                .invoke_app_hook("terminate-window", "")
                .expect("terminate window event");
            robot.wait_for_idle().expect("background update");
            robot
                .validate_content("Background")
                .expect("background content");

            robot
                .invoke_app_hook("init-window", "")
                .expect("init window event");
            robot.wait_for_present_frame().expect("first resumed frame");
            robot.validate_content("Resumed").expect("resumed content");

            robot.exit().expect("exit Android resume robot");
        })
        .run(resume_probe);
}

#[composable]
fn resume_probe() {
    let state = rememberMutableStateOf(|| ResumeState::Attached);
    RESUME_STATE.with(|slot| *slot.borrow_mut() = Some(state));
    let label = match state.get() {
        ResumeState::Attached => "Attached",
        ResumeState::Background => "Background",
        ResumeState::Resumed => "Resumed",
    };
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
        || {
            Text(label.to_string(), Modifier::empty(), TextStyle::default());
        },
    );
}
