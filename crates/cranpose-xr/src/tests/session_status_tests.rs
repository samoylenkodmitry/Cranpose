use cranpose::LifecycleState;

use crate::session_status::{SessionState, SessionStatus, StatusWatch};

const ALL_STATES: [SessionState; 8] = [
    SessionState::Idle,
    SessionState::Ready,
    SessionState::Synchronized,
    SessionState::Visible,
    SessionState::Focused,
    SessionState::Stopping,
    SessionState::LossPending,
    SessionState::Exiting,
];

#[test]
fn every_runtime_state_puts_the_app_in_one_status() {
    let statuses = ALL_STATES.map(SessionStatus::of);
    assert_eq!(
        statuses,
        [
            SessionStatus::Idle,
            SessionStatus::Hidden,
            SessionStatus::Hidden,
            SessionStatus::Visible,
            SessionStatus::Focused,
            SessionStatus::Idle,
            SessionStatus::Exit,
            SessionStatus::Exit,
        ]
    );
}

#[test]
fn only_a_focused_app_has_input_and_only_a_shown_app_has_layers() {
    for (state, runs, shows, listens) in [
        (SessionState::Idle, false, false, false),
        (SessionState::Ready, true, false, false),
        (SessionState::Synchronized, true, false, false),
        (SessionState::Visible, true, true, false),
        (SessionState::Focused, true, true, true),
        (SessionState::Stopping, false, false, false),
        (SessionState::Exiting, false, false, false),
    ] {
        let status = SessionStatus::of(state);
        assert_eq!(
            (status.runs(), status.shows(), status.listens()),
            (runs, shows, listens),
            "{state:?}"
        );
    }
}

#[test]
fn the_lifecycle_follows_the_session_as_compose_follows_a_window() {
    let lifecycles = [
        (SessionState::Idle, LifecycleState::Stopped),
        (SessionState::Ready, LifecycleState::Stopped),
        (SessionState::Synchronized, LifecycleState::Stopped),
        (SessionState::Visible, LifecycleState::Paused),
        (SessionState::Focused, LifecycleState::Resumed),
        (SessionState::Stopping, LifecycleState::Stopped),
        (SessionState::LossPending, LifecycleState::Destroyed),
        (SessionState::Exiting, LifecycleState::Destroyed),
    ];
    for (state, lifecycle) in lifecycles {
        assert_eq!(SessionStatus::of(state).lifecycle(), lifecycle, "{state:?}");
    }
}

#[test]
fn a_session_without_input_focus_gives_the_app_no_aim() {
    let aims = [Some(1_u8), Some(2)];
    for state in ALL_STATES {
        let status = SessionStatus::of(state);
        let expected = if state == SessionState::Focused {
            aims
        } else {
            [None, None]
        };
        assert_eq!(status.input(aims), expected, "{state:?}");
    }
    assert_eq!(
        SessionStatus::Focused.input([None, Some(2_u8)]),
        [None, Some(2)],
        "a controller that the runtime loses stays lost"
    );
}

#[test]
fn the_watch_reports_each_change_once_with_the_frames_of_the_status_it_leaves() {
    let mut watch = StatusWatch::default();
    assert_eq!(
        watch.change(SessionStatus::Idle),
        None,
        "a session that has not begun is no change"
    );
    let change = watch.change(SessionStatus::Hidden).unwrap();
    assert_eq!(
        (change.from, change.to, change.frames, change.lifecycle),
        (
            SessionStatus::Idle,
            SessionStatus::Hidden,
            0,
            LifecycleState::Stopped
        )
    );
    watch.change(SessionStatus::Focused).unwrap();
    for _ in 0..3 {
        assert_eq!(watch.change(SessionStatus::Focused), None);
        watch.ran();
    }
    let change = watch.change(SessionStatus::Visible).unwrap();
    assert_eq!(
        (change.from, change.to, change.frames, change.lifecycle),
        (
            SessionStatus::Focused,
            SessionStatus::Visible,
            3,
            LifecycleState::Paused
        )
    );
    watch.ran();
    let change = watch.change(SessionStatus::Focused).unwrap();
    assert_eq!(
        (change.from, change.frames, change.lifecycle),
        (SessionStatus::Visible, 1, LifecycleState::Resumed),
        "focus comes back and the count of the new status starts over"
    );
}

#[test]
fn a_session_that_stops_and_begins_again_passes_every_status_in_between() {
    // The order that a Quest reports when the activity pauses and resumes
    let order = [
        SessionState::Focused,
        SessionState::Synchronized,
        SessionState::Stopping,
        SessionState::Idle,
        SessionState::Ready,
        SessionState::Synchronized,
        SessionState::Visible,
        SessionState::Focused,
    ];
    let mut watch = StatusWatch::default();
    let lifecycles: Vec<_> = order
        .into_iter()
        .filter_map(|state| watch.change(SessionStatus::of(state)))
        .map(|change| change.lifecycle)
        .collect();
    // The repeated IDLE and the SYNCHRONIZED after READY change nothing. Hidden and idle are two
    // statuses with one lifecycle, which the host's dispatch folds into one transition.
    assert_eq!(
        lifecycles,
        [
            LifecycleState::Resumed,
            LifecycleState::Stopped,
            LifecycleState::Stopped,
            LifecycleState::Stopped,
            LifecycleState::Paused,
            LifecycleState::Resumed,
        ]
    );
}

#[test]
fn a_run_of_frames_that_never_changes_the_status_reports_no_change() {
    let mut watch = StatusWatch::default();
    watch.change(SessionStatus::Focused).unwrap();
    for _ in 0..1000 {
        watch.ran();
        assert_eq!(watch.change(SessionStatus::Focused), None);
    }
    let change = watch.change(SessionStatus::Exit).unwrap();
    assert_eq!(change.frames, 1000);
    assert_eq!(change.lifecycle, LifecycleState::Destroyed);
}
