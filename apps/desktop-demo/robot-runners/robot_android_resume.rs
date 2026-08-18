use cranpose::AppLauncher;
use cranpose_core::{useState, MutableState};
use cranpose_ui::{composable, Column, ColumnSpec, LinearArrangement, Modifier, Text, TextStyle};
use std::cell::RefCell;
use std::path::Path;

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

fn android_resume_contract_is_fixed() {
    let source_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/cranpose/src/android.rs");
    let source = std::fs::read_to_string(source_path).expect("read Android runtime source");
    assert!(
        !source.contains("gpu_resources = None;"),
        "TerminateWindow must retain the Android GPU device and renderer warm state"
    );
    assert!(
        source.contains("setup.resources.surface_dirty = true;\n            shell.mark_dirty();"),
        "InitWindow must schedule a fresh update before the first resumed present"
    );
}

fn main() {
    AppLauncher::new()
        .with_title("Android Resume")
        .with_size(640, 480)
        .with_headless(true)
        .with_robot_app_hook(lifecycle_hook)
        .with_test_driver(|robot| {
            robot.wait_for_idle().expect("initial frame");
            robot.validate_content("Attached").expect("initial content");

            robot
                .invoke_app_hook("terminate-window", "")
                .expect("terminate window event");
            robot.wait_for_idle().expect("background update");
            robot.validate_content("Background").expect("background content");

            robot
                .invoke_app_hook("init-window", "")
                .expect("init window event");
            robot
                .wait_for_present_frame()
                .expect("first resumed frame");
            robot.validate_content("Resumed").expect("resumed content");

            android_resume_contract_is_fixed();
            robot.exit().expect("exit Android resume robot");
        })
        .run(resume_probe);
}

#[composable]
#[allow(non_snake_case)]
fn resume_probe() {
    let state = useState(|| ResumeState::Attached);
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
