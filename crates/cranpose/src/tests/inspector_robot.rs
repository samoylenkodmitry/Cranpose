use super::*;

#[test]
fn inspector_robot_query_is_separate_and_propagates_failures() {
    let (channel, robot) = RobotChannel::new(|| {});
    let state = cranpose_app_shell::inspector::InspectorState {
        open: true,
        ..Default::default()
    };
    channel
        .tx
        .send(RobotResponse::InspectorState(Box::new(state.clone())))
        .expect("response");
    assert_eq!(robot.inspector_state().expect("snapshot"), state);
    assert!(matches!(
        channel.rx.try_recv(),
        Ok(RobotCommand::GetInspectorState)
    ));
    channel
        .tx
        .send(RobotResponse::Error("snapshot unavailable".into()))
        .expect("error response");
    assert_eq!(robot.inspector_state(), Err("snapshot unavailable".into()));
    drop(channel);
    assert!(robot.inspector_state().is_err());
}
