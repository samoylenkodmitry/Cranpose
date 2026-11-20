use super::*;

#[test]
fn outside_a_headset_every_capability_is_off() {
    let none = SpatialCapabilities::default();
    assert!(!none.is_spatial_ui_enabled());
    assert!(!none.is_passthrough_control_enabled());
    assert!(!none.is_spatial_audio_enabled());
}

#[test]
fn a_headset_has_spatial_audio_only_when_the_audio_player_has_a_scene() {
    let headset = SpatialCapabilities::in_headset(true);
    assert!(headset.is_spatial_ui_enabled());
    assert!(
        !headset.is_spatial_audio_enabled(),
        "no audio player has said so yet"
    );
    assert!(headset.with_spatial_audio(true).is_spatial_audio_enabled());
    assert!(
        !headset
            .with_spatial_audio(true)
            .with_spatial_audio(false)
            .is_spatial_audio_enabled()
    );
}
