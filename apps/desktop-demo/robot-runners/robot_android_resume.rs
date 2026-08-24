use std::{cell::RefCell, path::Path};

use cranpose::AppLauncher;
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

fn android_resume_contract_is_fixed() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/cranpose/src");
    let source = std::fs::read_to_string(source_root.join("android.rs"))
        .expect("read Android runtime source");
    assert!(
        !source.contains(
            "drop_present_surface(&mut gpu_resources, &mut app_shell);\n                            } else {\n                                gpu_resources = None;"
        ),
        "TerminateWindow must retain the Android GPU device and renderer warm state"
    );
    assert!(
        source.contains("resources.surface = None;"),
        "TerminateWindow must detach only the native surface"
    );
    assert!(
        source.contains("setup.resources.surface_dirty = true;\n            shell.mark_dirty();"),
        "InitWindow must schedule a fresh update before the first resumed present"
    );
    let vsync = std::fs::read_to_string(source_root.join("android_vsync.rs"))
        .expect("read Android vsync source");
    assert!(
        !vsync.contains("OnceLock<Box<dyn Fn() + Send + Sync>>"),
        "android_main relaunch must replace the vsync waker"
    );
    assert!(
        vsync.contains(
            "pub(crate) fn install_waker(waker: impl Fn() + Send + Sync + 'static) {\n    CALLBACK_POSTED.store(false, Ordering::Release);\n    UNAVAILABLE.store(false, Ordering::Release);"
        ),
        "android_main relaunch must reset pending and unavailable vsync state"
    );
    let accessibility = std::fs::read_to_string(source_root.join("android_accessibility.rs"))
        .expect("read Android accessibility source");
    assert!(
        !accessibility.contains("OnceLock<android_activity::AndroidAppWaker>"),
        "android_main relaunch must replace the accessibility waker"
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
            robot
                .validate_content("Background")
                .expect("background content");

            robot
                .invoke_app_hook("init-window", "")
                .expect("init window event");
            robot.wait_for_present_frame().expect("first resumed frame");
            robot.validate_content("Resumed").expect("resumed content");

            android_resume_contract_is_fixed();
            robot.exit().expect("exit Android resume robot");
        })
        .run(resume_probe);
}

#[composable]
#[allow(non_snake_case)]
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
