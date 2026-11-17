use std::{
    sync::atomic::{AtomicUsize, Ordering},
    task::Waker,
};

use super::*;
use crate::registry::test_service_guard;

// A platform that records the prompts it was asked for and answers when a
// test says so.
struct Platform {
    availability: DeviceAuthAvailability,
    prompts: AtomicUsize,
    last: Mutex<Option<(DeviceAuthRequest, String)>>,
    // The outcome given at once, as a backend whose prompt cannot open.
    at_once: Option<DeviceAuthOutcome>,
}

impl Platform {
    fn new(availability: DeviceAuthAvailability) -> Arc<Self> {
        Arc::new(Self {
            availability,
            prompts: AtomicUsize::new(0),
            last: Mutex::new(None),
            at_once: None,
        })
    }

    fn last_request(&self) -> DeviceAuthRequest {
        self.last.lock().as_ref().expect("a prompt").0
    }
}

impl DeviceAuthenticator for Platform {
    fn availability(&self) -> DeviceAuthAvailability {
        self.availability
    }

    fn authenticate(&self, request: DeviceAuthRequest, reason: &str) {
        self.prompts.fetch_add(1, Ordering::SeqCst);
        *self.last.lock() = Some((request, reason.to_owned()));
        if let Some(outcome) = self.at_once {
            finish_device_auth(request, outcome);
        }
    }
}

fn poll(future: &mut DeviceAuthFuture) -> Poll<DeviceAuthOutcome> {
    Pin::new(future).poll(&mut Context::from_waker(Waker::noop()))
}

#[test]
fn without_a_backend_nothing_unlocks() {
    let _guard = test_service_guard();
    assert_eq!(
        device_auth_availability(),
        DeviceAuthAvailability::Unsupported
    );
    assert!(!device_auth_availability().can_authenticate());
    assert_eq!(
        pollster::block_on(authenticate_device_owner("Open the history")),
        DeviceAuthOutcome::Unavailable
    );
}

#[test]
fn the_backend_says_what_the_device_can_ask_with() {
    let _guard = test_service_guard();
    set_platform_device_authenticator(Some(Platform::new(DeviceAuthAvailability::Biometric)));
    assert_eq!(
        device_auth_availability(),
        DeviceAuthAvailability::Biometric
    );
    assert!(DeviceAuthAvailability::Credential.can_authenticate());
    assert!(!DeviceAuthAvailability::NoScreenLock.can_authenticate());
}

#[test]
fn the_future_ends_with_the_answer_of_the_prompt() {
    let _guard = test_service_guard();
    let platform = Platform::new(DeviceAuthAvailability::Biometric);
    set_platform_device_authenticator(Some(Arc::clone(&platform) as Arc<dyn DeviceAuthenticator>));
    let mut future = authenticate_device_owner("Open the history");
    assert_eq!(poll(&mut future), Poll::Pending);
    let (request, reason) = platform.last.lock().clone().expect("a prompt");
    assert_eq!(reason, "Open the history");
    finish_device_auth(request, DeviceAuthOutcome::Unlocked);
    assert_eq!(poll(&mut future), Poll::Ready(DeviceAuthOutcome::Unlocked));
}

#[test]
fn a_second_call_waits_for_the_open_prompt() {
    let _guard = test_service_guard();
    let platform = Platform::new(DeviceAuthAvailability::Credential);
    set_platform_device_authenticator(Some(Arc::clone(&platform) as Arc<dyn DeviceAuthenticator>));
    let mut first = authenticate_device_owner("Open the history");
    let mut second = authenticate_device_owner("Open the history");
    assert_eq!(platform.prompts.load(Ordering::SeqCst), 1);
    assert_eq!(poll(&mut second), Poll::Pending);
    finish_device_auth(platform.last_request(), DeviceAuthOutcome::Cancelled);
    assert_eq!(poll(&mut first), Poll::Ready(DeviceAuthOutcome::Cancelled));
    assert_eq!(poll(&mut second), Poll::Ready(DeviceAuthOutcome::Cancelled));
    // Once it closed, the next call opens a new prompt.
    let _third = authenticate_device_owner("Open the history");
    assert_eq!(platform.prompts.load(Ordering::SeqCst), 2);
    finish_device_auth(platform.last_request(), DeviceAuthOutcome::Cancelled);
}

#[test]
fn an_answer_for_a_closed_prompt_changes_nothing() {
    let _guard = test_service_guard();
    let platform = Platform::new(DeviceAuthAvailability::Biometric);
    set_platform_device_authenticator(Some(Arc::clone(&platform) as Arc<dyn DeviceAuthenticator>));
    let mut first = authenticate_device_owner("Open the history");
    let old = platform.last_request();
    finish_device_auth(old, DeviceAuthOutcome::Cancelled);
    assert_eq!(poll(&mut first), Poll::Ready(DeviceAuthOutcome::Cancelled));
    let mut second = authenticate_device_owner("Open the history");
    // A late or repeated answer to the first prompt does not unlock the second.
    finish_device_auth(old, DeviceAuthOutcome::Unlocked);
    assert_eq!(poll(&mut second), Poll::Pending);
    finish_device_auth(platform.last_request(), DeviceAuthOutcome::Failed);
    assert_eq!(poll(&mut second), Poll::Ready(DeviceAuthOutcome::Failed));
}

#[test]
fn a_prompt_that_cannot_open_ends_at_once() {
    let _guard = test_service_guard();
    set_platform_device_authenticator(Some(Arc::new(Platform {
        availability: DeviceAuthAvailability::NoScreenLock,
        prompts: AtomicUsize::new(0),
        last: Mutex::new(None),
        at_once: Some(DeviceAuthOutcome::Unavailable),
    })));
    assert_eq!(
        pollster::block_on(authenticate_device_owner("Open the history")),
        DeviceAuthOutcome::Unavailable
    );
}

#[test]
fn an_empty_reason_shows_no_prompt() {
    let _guard = test_service_guard();
    let platform = Platform::new(DeviceAuthAvailability::Biometric);
    set_platform_device_authenticator(Some(Arc::clone(&platform) as Arc<dyn DeviceAuthenticator>));
    assert_eq!(
        pollster::block_on(authenticate_device_owner("  ")),
        DeviceAuthOutcome::Failed
    );
    assert_eq!(platform.prompts.load(Ordering::SeqCst), 0);
}

#[test]
fn removing_the_backend_ends_the_open_prompt() {
    let _guard = test_service_guard();
    set_platform_device_authenticator(Some(Platform::new(DeviceAuthAvailability::Biometric)));
    let mut future = authenticate_device_owner("Open the history");
    assert_eq!(poll(&mut future), Poll::Pending);
    set_platform_device_authenticator(None);
    assert_eq!(
        poll(&mut future),
        Poll::Ready(DeviceAuthOutcome::Unavailable)
    );
}

// Counts the wakes the future asks for.
struct Wakes(AtomicUsize);

impl std::task::Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn an_answer_from_another_thread_wakes_the_future() {
    let _guard = test_service_guard();
    let platform = Platform::new(DeviceAuthAvailability::Biometric);
    set_platform_device_authenticator(Some(Arc::clone(&platform) as Arc<dyn DeviceAuthenticator>));
    let mut future = authenticate_device_owner("Open the history");
    let wakes = Arc::new(Wakes(AtomicUsize::new(0)));
    let waker = Waker::from(Arc::clone(&wakes));
    let mut context = Context::from_waker(&waker);
    assert_eq!(Pin::new(&mut future).poll(&mut context), Poll::Pending);
    let request = platform.last_request();
    std::thread::spawn(move || finish_device_auth(request, DeviceAuthOutcome::Unlocked))
        .join()
        .expect("the answering thread");
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        Pin::new(&mut future).poll(&mut context),
        Poll::Ready(DeviceAuthOutcome::Unlocked)
    );
}
