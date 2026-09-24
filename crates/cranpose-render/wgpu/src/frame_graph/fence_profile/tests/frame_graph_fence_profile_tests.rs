use super::*;

#[test]
fn unfinished_frame_discards_its_commands_and_label() {
    let (_lock, device, _queue) = super::super::upload_test_device();
    let recording = FrameRecording;
    PROFILE.with(|profile| {
        let mut profile = profile.borrow_mut();
        profile.current_label = Some("abandoned".to_owned());
        profile.pending.push((None, new_encoder(&device).finish()));
    });
    drop(recording);
    PROFILE.with(|profile| {
        let profile = profile.borrow();
        assert!(profile.pending.is_empty());
        assert!(profile.current_label.is_none());
    });
}
