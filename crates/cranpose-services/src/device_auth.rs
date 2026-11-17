//! Asking the owner of the device to unlock: with a face, a fingerprint, or
//! the PIN, pattern or password of the screen lock.
//!
//! An application uses it to keep part of itself, such as a history, from a
//! person holding the phone who is not its owner. The system shows its own
//! prompt and tells the application only whether the owner unlocked it; the
//! application never sees a face, a finger or a code.
//!
//! [`device_auth_availability`] says what the device can ask with, and never
//! prompts. [`authenticate_device_owner`] shows the prompt with the reason the
//! application gives, and its future ends once the prompt closes.
//!
//! On Android the backend is in a build only when its build script declares
//! `cranpose_capabilities::Use::device_auth`, which brings the
//! `USE_BIOMETRIC` permission. On iOS the `cranpose` feature `device-auth`
//! links LocalAuthentication, and the same declaration gives Face ID the
//! sentence it shows the first time (`NSFaceIDUsageDescription`). Where no
//! backend is installed (desktop systems, watches, the web, or a build that
//! did not declare the service), the availability is
//! [`DeviceAuthAvailability::Unsupported`] and every prompt ends at once with
//! [`DeviceAuthOutcome::Unavailable`]: nothing is ever unlocked without a
//! platform to ask.

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

use cranpose_core::ProcessIds;
use parking_lot::Mutex;

use crate::registry::ServiceRegistry;

/// What the device can ask its owner with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DeviceAuthAvailability {
    /// This platform or this build has no way to ask.
    #[default]
    Unsupported,
    /// The device has no screen lock, so it has nothing to ask for. The
    /// person sets one in the system settings.
    NoScreenLock,
    /// The PIN, pattern or password of the screen lock.
    Credential,
    /// A face or a fingerprint, with the screen lock when it fails.
    Biometric,
}

impl DeviceAuthAvailability {
    /// Whether [`authenticate_device_owner`] can show a prompt.
    pub const fn can_authenticate(self) -> bool {
        matches!(self, Self::Credential | Self::Biometric)
    }
}

/// How a prompt ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceAuthOutcome {
    /// The owner unlocked.
    Unlocked,
    /// The person closed the prompt, or the system closed it, for example
    /// when the application went to the background.
    Cancelled,
    /// The prompt ended without an unlock: too many attempts, or an error
    /// of the platform.
    Failed,
    /// Nothing to ask with: no backend, or no screen lock.
    Unavailable,
}

/// One prompt, from [`authenticate_device_owner`] to the backend and back
/// through [`finish_device_auth`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeviceAuthRequest(u64);

impl DeviceAuthRequest {
    /// The number a backend carries through its platform's callbacks.
    pub const fn id(self) -> u64 {
        self.0
    }

    /// The request a platform callback names by [`id`](Self::id).
    pub const fn from_id(id: u64) -> Self {
        Self(id)
    }
}

/// A platform's way of asking the owner, installed by the platform backend.
pub trait DeviceAuthenticator: Send + Sync {
    /// What the device can ask with now. It never prompts.
    fn availability(&self) -> DeviceAuthAvailability;

    /// Shows the system prompt with `reason`, which is never empty. The
    /// backend calls [`finish_device_auth`] with `request` exactly once,
    /// when the prompt closes or cannot open.
    fn authenticate(&self, request: DeviceAuthRequest, reason: &str);
}

static PLATFORM: ServiceRegistry<dyn DeviceAuthenticator> = ServiceRegistry::new();

static NEXT_REQUEST: ProcessIds = ProcessIds::starting_at(1);

// The answer one prompt gives every future that waits for it.
#[derive(Default)]
struct Answer {
    outcome: Option<DeviceAuthOutcome>,
    wakers: Vec<Waker>,
}

impl Answer {
    fn ready(outcome: DeviceAuthOutcome) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            outcome: Some(outcome),
            wakers: Vec::new(),
        }))
    }
}

// The prompt that is open now, with its answer.
static OPEN: Mutex<Option<(DeviceAuthRequest, Arc<Mutex<Answer>>)>> = Mutex::new(None);

/// Installs the platform's backend, or removes it with `None`. Platform
/// backends call it. Removing the backend ends an open prompt with
/// [`DeviceAuthOutcome::Unavailable`], since no one is left to answer it.
pub fn set_platform_device_authenticator(authenticator: Option<Arc<dyn DeviceAuthenticator>>) {
    match authenticator {
        Some(authenticator) => PLATFORM.set(authenticator),
        None => {
            PLATFORM.clear();
            let open = OPEN.lock().as_ref().map(|(request, _)| *request);
            if let Some(request) = open {
                finish_device_auth(request, DeviceAuthOutcome::Unavailable);
            }
        }
    }
}

/// What the device can ask its owner with now. It never prompts.
pub fn device_auth_availability() -> DeviceAuthAvailability {
    PLATFORM
        .get()
        .map_or(DeviceAuthAvailability::Unsupported, |platform| {
            platform.availability()
        })
}

/// Asks the owner of the device to unlock, with `reason` on the prompt.
///
/// The future ends when the prompt closes. One prompt is open at a time: a
/// call while one is open shows no second prompt, and its future ends with
/// the open one's outcome. Dropping the future leaves the prompt open; its
/// outcome then goes to no one.
///
/// Android shows `reason` as the title of the prompt, and iOS below the name
/// of the application. An empty `reason` shows nothing and ends with
/// [`DeviceAuthOutcome::Failed`], since both platforms refuse one.
pub fn authenticate_device_owner(reason: &str) -> DeviceAuthFuture {
    let Some(platform) = PLATFORM.get() else {
        return DeviceAuthFuture::ready(DeviceAuthOutcome::Unavailable);
    };
    if reason.trim().is_empty() {
        log::warn!("cranpose: a device unlock was asked with no reason to show");
        return DeviceAuthFuture::ready(DeviceAuthOutcome::Failed);
    }
    let mut open = OPEN.lock();
    if let Some((_, answer)) = open.as_ref() {
        return DeviceAuthFuture {
            answer: Arc::clone(answer),
        };
    }
    let request = DeviceAuthRequest(NEXT_REQUEST.next());
    let answer = Arc::new(Mutex::new(Answer::default()));
    *open = Some((request, Arc::clone(&answer)));
    // A backend that cannot open its prompt finishes at once, which takes
    // this lock.
    drop(open);
    platform.authenticate(request, reason);
    DeviceAuthFuture { answer }
}

/// Ends the prompt of `request` with `outcome`. Backends call it once per
/// request, from any thread; a request that is no longer open is ignored.
pub fn finish_device_auth(request: DeviceAuthRequest, outcome: DeviceAuthOutcome) {
    let answer = {
        let mut open = OPEN.lock();
        match open.as_ref() {
            Some((open_request, _)) if *open_request == request => open.take().map(|(_, a)| a),
            _ => None,
        }
    };
    let Some(answer) = answer else {
        log::debug!("cranpose: a device unlock answer came for a closed prompt");
        return;
    };
    let wakers = {
        let mut answer = answer.lock();
        answer.outcome = Some(outcome);
        std::mem::take(&mut answer.wakers)
    };
    for waker in wakers {
        waker.wake();
    }
}

/// The outcome of a prompt, from [`authenticate_device_owner`].
#[must_use = "the outcome says whether the owner unlocked"]
pub struct DeviceAuthFuture {
    answer: Arc<Mutex<Answer>>,
}

impl DeviceAuthFuture {
    fn ready(outcome: DeviceAuthOutcome) -> Self {
        Self {
            answer: Answer::ready(outcome),
        }
    }
}

impl std::fmt::Debug for DeviceAuthFuture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DeviceAuthFuture")
            .field("outcome", &self.answer.lock().outcome)
            .finish()
    }
}

impl Future for DeviceAuthFuture {
    type Output = DeviceAuthOutcome;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<DeviceAuthOutcome> {
        let mut answer = self.answer.lock();
        if let Some(outcome) = answer.outcome {
            return Poll::Ready(outcome);
        }
        let waker = context.waker();
        if !answer.wakers.iter().any(|known| known.will_wake(waker)) {
            answer.wakers.push(waker.clone());
        }
        Poll::Pending
    }
}

/// What the device can ask with, read again each time the application's
/// lifecycle changes, such as on its return from the system settings where
/// the person set a screen lock. It never prompts.
#[expect(non_snake_case, reason = "names follow Compose's casing")]
#[track_caller]
pub fn rememberDeviceAuthAvailability() -> DeviceAuthAvailability {
    let lifecycle = crate::rememberLifecycleState().get();
    cranpose_core::rememberKeyed(lifecycle, |_| device_auth_availability())
}

#[cfg(test)]
#[path = "tests/device_auth_tests.rs"]
mod tests;
