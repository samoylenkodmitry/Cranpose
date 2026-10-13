use super::*;

#[test]
fn unfinished_frame_discards_its_commands_and_label() {
    let (_lock, device, _queue) = super::super::upload_test_device();
    let recording = FrameRecording;
    crate::thread_state::with(|state| {
        let mut profile = state.fence_profile.borrow_mut();
        profile.current_label = Some("abandoned".to_owned());
        profile.pending.push((None, new_encoder(&device).finish()));
    });
    drop(recording);
    crate::thread_state::with(|state| {
        let profile = state.fence_profile.borrow();
        assert!(profile.pending.is_empty());
        assert!(profile.current_label.is_none());
    });
}
