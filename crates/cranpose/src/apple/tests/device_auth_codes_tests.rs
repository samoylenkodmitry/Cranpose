use super::*;

#[test]
fn closing_the_prompt_is_a_cancel() {
    for code in [USER_CANCEL, USER_FALLBACK, SYSTEM_CANCEL, APP_CANCEL] {
        assert_eq!(outcome(Some(code)), DeviceAuthOutcome::Cancelled);
    }
}

#[test]
fn a_device_without_a_passcode_is_unavailable() {
    assert_eq!(
        outcome(Some(PASSCODE_NOT_SET)),
        DeviceAuthOutcome::Unavailable
    );
}

#[test]
fn every_other_error_fails() {
    // AuthenticationFailed, BiometryLockout, InvalidContext, NotInteractive.
    for code in [AUTHENTICATION_FAILED, -8, -10, -1004] {
        assert_eq!(outcome(Some(code)), DeviceAuthOutcome::Failed);
    }
    assert_eq!(outcome(None), DeviceAuthOutcome::Failed);
}
