//! LocalAuthentication's error codes, read as the unlock's outcomes. Kept
//! apart from the Objective-C so the host tests read them.

use cranpose_services::DeviceAuthOutcome;

// `LAErrorAuthenticationFailed`.
const AUTHENTICATION_FAILED: isize = -1;
// `LAErrorUserCancel`.
const USER_CANCEL: isize = -2;
// `LAErrorUserFallback`.
const USER_FALLBACK: isize = -3;
// `LAErrorSystemCancel`: another application came to the front.
const SYSTEM_CANCEL: isize = -4;
// `LAErrorPasscodeNotSet`.
pub(crate) const PASSCODE_NOT_SET: isize = -5;
// `LAErrorAppCancel`.
const APP_CANCEL: isize = -9;

// How an evaluation that did not unlock ended, from the code of its error
// in `LAErrorDomain`, or `None` for an error of another domain or none.
pub(crate) fn outcome(code: Option<isize>) -> DeviceAuthOutcome {
    match code {
        Some(USER_CANCEL | USER_FALLBACK | SYSTEM_CANCEL | APP_CANCEL) => {
            DeviceAuthOutcome::Cancelled
        }
        Some(PASSCODE_NOT_SET) => DeviceAuthOutcome::Unavailable,
        // AUTHENTICATION_FAILED, a lockout, an invalid context and the rest.
        Some(AUTHENTICATION_FAILED | _) | None => DeviceAuthOutcome::Failed,
    }
}

#[cfg(test)]
#[path = "tests/device_auth_codes_tests.rs"]
mod tests;
