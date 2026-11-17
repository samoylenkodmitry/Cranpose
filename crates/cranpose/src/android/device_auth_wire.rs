//! The numbers `CranposeDeviceAuth` and `CranposeActivity` hand Rust about
//! the unlock prompt, read as the service's values. Kept apart from the JNI
//! so the host tests read them.

use cranpose_services::{DeviceAuthAvailability, DeviceAuthOutcome};

// The request code the confirm-credential screen starts with on Android 9
// and older. It lies outside `CranposeActivity`'s `REQUEST_BASE`, which the
// pickers own; `CranposeDeviceAuth.CONFIRM_CREDENTIAL_REQUEST` is the same
// number.
pub(crate) const CONFIRM_CREDENTIAL_REQUEST: i32 = 0x0C9B_0001;

// `Activity.RESULT_OK`.
const RESULT_OK: i32 = -1;

// The prompt unlocked.
pub(crate) const KIND_SUCCEEDED: i32 = 0;
// The prompt ended with a `BiometricPrompt` error code.
pub(crate) const KIND_ERROR: i32 = 1;
// The prompt could not open.
pub(crate) const KIND_NOT_SHOWN: i32 = 2;
// The device has no screen lock to ask for.
pub(crate) const KIND_NO_SCREEN_LOCK: i32 = 3;

// What `cranposeDeviceAuthAvailability` returned.
pub(crate) fn availability(code: i32) -> DeviceAuthAvailability {
    match code {
        0 => DeviceAuthAvailability::NoScreenLock,
        1 => DeviceAuthAvailability::Credential,
        2 => DeviceAuthAvailability::Biometric,
        _ => DeviceAuthAvailability::Unsupported,
    }
}

// How the prompt ended, from the kind `nativeOnDeviceAuthResult` names and
// its code.
pub(crate) fn outcome(kind: i32, code: i32) -> DeviceAuthOutcome {
    match kind {
        KIND_SUCCEEDED => DeviceAuthOutcome::Unlocked,
        KIND_ERROR => prompt_error(code),
        KIND_NO_SCREEN_LOCK => DeviceAuthOutcome::Unavailable,
        KIND_NOT_SHOWN => DeviceAuthOutcome::Failed,
        _ => DeviceAuthOutcome::Failed,
    }
}

// A `BiometricPrompt.BIOMETRIC_ERROR_*` code. A face or finger that did not
// match is no error: the prompt stays open for the next try.
fn prompt_error(code: i32) -> DeviceAuthOutcome {
    match code {
        // TIMEOUT, CANCELED (the system closed it, as when the application
        // left the screen), USER_CANCELED, NEGATIVE_BUTTON.
        3 | 5 | 10 | 13 => DeviceAuthOutcome::Cancelled,
        // NO_BIOMETRICS, HW_NOT_PRESENT, NO_DEVICE_CREDENTIAL: nothing to
        // ask with.
        11 | 12 | 14 => DeviceAuthOutcome::Unavailable,
        // LOCKOUT, LOCKOUT_PERMANENT, HW_UNAVAILABLE, VENDOR and the rest.
        _ => DeviceAuthOutcome::Failed,
    }
}

// How the confirm-credential screen ended, from its activity result.
pub(crate) fn confirmed(result_code: i32) -> DeviceAuthOutcome {
    if result_code == RESULT_OK {
        DeviceAuthOutcome::Unlocked
    } else {
        DeviceAuthOutcome::Cancelled
    }
}

#[cfg(test)]
#[path = "tests/device_auth_wire_tests.rs"]
mod tests;
