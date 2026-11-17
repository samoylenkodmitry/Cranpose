#![expect(unsafe_code, reason = "Objective-C messages to LocalAuthentication")]

//! Asking the owner to unlock on iOS through LocalAuthentication: Face ID or
//! Touch ID, with the passcode when they fail or are not enrolled.

use std::sync::Arc;

use block2::RcBlock;
use cranpose_services::{
    DeviceAuthAvailability, DeviceAuthOutcome, DeviceAuthRequest, DeviceAuthenticator,
    finish_device_auth, set_platform_device_authenticator,
};
use dispatch2::DispatchQueue;
use objc2::runtime::Bool;
use objc2_foundation::{NSError, NSString};
use objc2_local_authentication::{LAContext, LAErrorDomain, LAPolicy};

use super::device_auth_codes::{PASSCODE_NOT_SET, outcome};

pub(crate) fn register() {
    set_platform_device_authenticator(Some(Arc::new(AppleDeviceAuth)));
}

struct AppleDeviceAuth;

// The code of `error` when it is LocalAuthentication's own.
fn local_code(error: &NSError) -> Option<isize> {
    // SAFETY: LAErrorDomain is a constant string of the framework, valid for
    // the life of the process.
    let domain = unsafe { LAErrorDomain };
    (&*error.domain() == domain).then(|| error.code())
}

impl DeviceAuthenticator for AppleDeviceAuth {
    fn availability(&self) -> DeviceAuthAvailability {
        // SAFETY: a fresh context; asking whether a policy can be evaluated
        // shows nothing.
        let context = unsafe { LAContext::new() };
        // SAFETY: as above.
        if let Err(error) =
            unsafe { context.canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthentication) }
        {
            return if local_code(&error) == Some(PASSCODE_NOT_SET) {
                DeviceAuthAvailability::NoScreenLock
            } else {
                DeviceAuthAvailability::Unsupported
            };
        }
        // SAFETY: as above.
        let biometric = unsafe {
            context.canEvaluatePolicy_error(LAPolicy::DeviceOwnerAuthenticationWithBiometrics)
        };
        if biometric.is_ok() {
            DeviceAuthAvailability::Biometric
        } else {
            DeviceAuthAvailability::Credential
        }
    }

    fn authenticate(&self, request: DeviceAuthRequest, reason: &str) {
        // SAFETY: a fresh context, owned by this prompt alone.
        let context = unsafe { LAContext::new() };
        let reason = NSString::from_str(reason);
        // The reply holds the context until the prompt ends: a context that
        // is released is invalidated, and its prompt with it.
        let held = context.clone();
        let reply = RcBlock::new(move |unlocked: Bool, error: *mut NSError| {
            let outcome = if unlocked.as_bool() {
                DeviceAuthOutcome::Unlocked
            } else {
                // SAFETY: the framework passes a valid error or null.
                outcome(unsafe { error.as_ref() }.and_then(local_code))
            };
            // The answer comes on a queue of the framework; the application
            // reads it on the main thread, as it reads every other answer.
            DispatchQueue::main().exec_async(move || finish_device_auth(request, outcome));
            let _ = &held;
        });
        // SAFETY: the policy, the reason and the reply are valid for the
        // call. The system copies the reply and calls it once, on a queue of
        // its own; what it holds (the context, a request id) may be used and
        // released from any thread.
        unsafe {
            context.evaluatePolicy_localizedReason_reply(
                LAPolicy::DeviceOwnerAuthentication,
                &reason,
                &reply,
            );
        }
    }
}
