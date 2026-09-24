use cranpose_services::{clear_platform_audio, default_audio};
use parking_lot::{Mutex, MutexGuard};

use super::*;

fn platform_audio_guard() -> MutexGuard<'static, ()> {
    static PLATFORM_AUDIO_LOCK: Mutex<()> = Mutex::new(());
    PLATFORM_AUDIO_LOCK.lock()
}

#[test]
fn install_registers_the_engine_as_the_platform_player() {
    let _guard = platform_audio_guard();
    clear_platform_audio();
    assert!(!default_audio().is_available());
    install();
    assert_eq!(default_audio().is_available(), has_device_backend());
    clear_platform_audio();
    assert!(!default_audio().is_available());
}

#[test]
fn create_does_not_register_anything() {
    let _guard = platform_audio_guard();
    clear_platform_audio();
    let engine = create();
    assert!(!engine.is_running());
    assert!(!default_audio().is_available());
}

#[test]
fn install_reuses_the_registered_engine() {
    let _guard = platform_audio_guard();
    let first = install();
    let second = install();

    assert!(Arc::ptr_eq(&first, &second));
}
