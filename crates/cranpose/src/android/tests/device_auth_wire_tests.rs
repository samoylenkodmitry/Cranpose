use super::*;

#[test]
fn availability_reads_each_code() {
    assert_eq!(availability(0), DeviceAuthAvailability::NoScreenLock);
    assert_eq!(availability(1), DeviceAuthAvailability::Credential);
    assert_eq!(availability(2), DeviceAuthAvailability::Biometric);
    assert_eq!(availability(-1), DeviceAuthAvailability::Unsupported);
}

#[test]
fn only_success_unlocks() {
    assert_eq!(outcome(KIND_SUCCEEDED, 0), DeviceAuthOutcome::Unlocked);
    for code in 0..32 {
        assert_ne!(outcome(KIND_ERROR, code), DeviceAuthOutcome::Unlocked);
    }
    assert_eq!(outcome(KIND_NOT_SHOWN, 0), DeviceAuthOutcome::Failed);
    assert_eq!(
        outcome(KIND_NO_SCREEN_LOCK, 0),
        DeviceAuthOutcome::Unavailable
    );
    assert_eq!(outcome(99, 0), DeviceAuthOutcome::Failed);
}

#[test]
fn closing_the_prompt_is_a_cancel() {
    // TIMEOUT, CANCELED, USER_CANCELED, NEGATIVE_BUTTON.
    for code in [3, 5, 10, 13] {
        assert_eq!(outcome(KIND_ERROR, code), DeviceAuthOutcome::Cancelled);
    }
}

#[test]
fn a_device_with_nothing_to_ask_with_is_unavailable() {
    // NO_BIOMETRICS, HW_NOT_PRESENT, NO_DEVICE_CREDENTIAL.
    for code in [11, 12, 14] {
        assert_eq!(outcome(KIND_ERROR, code), DeviceAuthOutcome::Unavailable);
    }
}

#[test]
fn a_lockout_fails() {
    // LOCKOUT, LOCKOUT_PERMANENT, HW_UNAVAILABLE.
    for code in [7, 9, 1] {
        assert_eq!(outcome(KIND_ERROR, code), DeviceAuthOutcome::Failed);
    }
}

#[test]
fn the_credential_screen_unlocks_only_on_ok() {
    assert_eq!(confirmed(-1), DeviceAuthOutcome::Unlocked);
    assert_eq!(confirmed(0), DeviceAuthOutcome::Cancelled);
    assert_eq!(confirmed(1), DeviceAuthOutcome::Cancelled);
}

#[test]
fn the_credential_request_is_not_a_picker_request() {
    // CranposeActivity's pickers own every code under REQUEST_BASE.
    assert_ne!(CONFIRM_CREDENTIAL_REQUEST & !0xFFFF, 0x0C9A_0000);
}
