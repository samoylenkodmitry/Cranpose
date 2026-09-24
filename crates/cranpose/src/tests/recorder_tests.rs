use std::{io::Read, path::PathBuf};

use super::*;

fn workspace_test_output_path(filename: &str) -> PathBuf {
    crate::test_scratch_dir("recorder").join(filename)
}

#[test]
fn generated_timestamp_uses_unix_millis() {
    let timestamp = generated_timestamp();
    assert!(timestamp.starts_with("unix_ms_"));
    assert_ne!(timestamp, "timestamp");
    assert!(
        timestamp["unix_ms_".len()..]
            .chars()
            .all(|ch| ch.is_ascii_digit()),
        "timestamp should contain only decimal milliseconds after the prefix: {timestamp}"
    );
}

#[test]
fn test_recorder_generates_data_driven_code() {
    let temp_path = workspace_test_output_path("test_recording.rs");
    {
        let mut recorder = InputRecorder::new(&temp_path);
        recorder.record_mouse_move(100.0, 200.0);
        recorder.record_mouse_down();
        recorder.record_mouse_up();
    }

    let mut content = String::new();
    std::fs::File::open(&temp_path)
        .unwrap()
        .read_to_string(&mut content)
        .unwrap();

    assert!(content.contains("use cranpose::recorder::{RobotAction, execute_actions};"));
    assert!(content.contains("//! Generated at: unix_ms_"));
    assert!(!content.contains("//! Generated at: timestamp"));
    assert!(content.contains("const ACTIONS: &[RobotAction] = &["));
    assert!(content.contains("MouseMove(100.0, 200.0)"));
    assert!(content.contains("MouseDown,"));
    assert!(content.contains("MouseUp,"));
    assert!(content.contains("execute_actions(&robot, ACTIONS);"));

    std::fs::remove_file(&temp_path).ok();
}

#[test]
fn test_events_to_actions_conversion() {
    let mut recorder = InputRecorder::new("/dev/null");

    recorder.events.push(RecordedEvent::MouseMove {
        time_ms: 0,
        x: 100.0,
        y: 200.0,
    });
    recorder
        .events
        .push(RecordedEvent::MouseDown { time_ms: 50 });
    recorder.events.push(RecordedEvent::MouseMove {
        time_ms: 60,
        x: 150.0,
        y: 250.0,
    });
    recorder
        .events
        .push(RecordedEvent::MouseUp { time_ms: 100 });

    let actions = recorder.events_to_actions();

    assert_eq!(actions.len(), 7);
    assert!(matches!(actions[0], RobotAction::MouseMove(100.0, 200.0)));
    assert!(matches!(actions[1], RobotAction::Sleep(50)));
    assert!(matches!(actions[2], RobotAction::MouseDown));
    assert!(matches!(actions[3], RobotAction::Sleep(10)));
    assert!(matches!(actions[4], RobotAction::MouseMove(150.0, 250.0)));
    assert!(matches!(actions[5], RobotAction::Sleep(40)));
    assert!(matches!(actions[6], RobotAction::MouseUp));
}

#[test]
fn test_robot_action_enum() {
    let actions = [
        RobotAction::Sleep(100),
        RobotAction::MouseMove(50.0, 75.5),
        RobotAction::MouseDown,
        RobotAction::MouseUp,
        RobotAction::Key("Enter".to_string()),
    ];

    assert_eq!(actions.len(), 5);
    assert_eq!(actions[0], RobotAction::Sleep(100));
    assert_eq!(actions[1], RobotAction::MouseMove(50.0, 75.5));
    assert_eq!(actions[2], RobotAction::MouseDown);
    assert_eq!(actions[3], RobotAction::MouseUp);
    assert_eq!(actions[4], RobotAction::Key("Enter".to_string()));
}
