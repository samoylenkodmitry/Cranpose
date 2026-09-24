use super::*;

#[test]
fn a_build_without_a_device_reports_unsupported() {
    if is_compiled() {
        return;
    }
    let (_command_tx, command_rx) = crate::ring::channel(4);
    let (retired_tx, _retired_rx) = crate::ring::channel(4);
    let seed = MixerSeed {
        commands: command_rx,
        retired: retired_tx,
        leaked_clips: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        underruns: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
        streaming: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    assert!(matches!(open_mixer(seed), Err(AudioError::Unsupported)));
}
