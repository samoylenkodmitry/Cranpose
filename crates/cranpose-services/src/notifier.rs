//! Local notifications: ask permission and post user-visible notifications.
//!
//! The compiled-in default is a no-op; platform backends install a real
//! notifier through [`set_platform_notifier`] (iOS `UNUserNotificationCenter`,
//! Android `NotificationManager`, desktop `notify-rust`, the web Notifications
//! API). Desktop/CI without a notification service simply drop them.

use cranpose_core::compositionLocalOfWithPolicy;
use cranpose_core::CompositionLocal;
use cranpose_core::CompositionLocalProvider;
use cranpose_macros::composable;
use std::cell::RefCell;
use std::rc::Rc;

/// A local notification to post.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotifyRequest {
    /// Stable identifier: re-posting with the same id replaces the previous
    /// notification (used for progress updates).
    pub id: String,
    pub title: String,
    pub body: String,
    /// Whether this is an ongoing/progress notification (best-effort; platforms
    /// without ongoing notifications post a normal one).
    pub ongoing: bool,
    /// Optional deep-link payload delivered to the app when the user taps it.
    pub deeplink: Option<String>,
}

impl NotifyRequest {
    pub fn new(id: impl Into<String>, title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            body: body.into(),
            ongoing: false,
            deeplink: None,
        }
    }

    pub fn ongoing(mut self, ongoing: bool) -> Self {
        self.ongoing = ongoing;
        self
    }

    pub fn with_deeplink(mut self, deeplink: impl Into<String>) -> Self {
        self.deeplink = Some(deeplink.into());
        self
    }
}

/// Posts local notifications. Installed by the platform backend; the default is
/// a no-op.
pub trait Notifier {
    /// Request permission to show notifications (idempotent; safe to call more
    /// than once).
    fn request_permission(&self);
    /// Post (or replace, by id) a local notification.
    fn notify(&self, request: NotifyRequest);
    /// Remove a delivered or pending notification by id.
    fn cancel(&self, id: &str);
}

pub type NotifierRef = Rc<dyn Notifier>;

struct NoopNotifier;

impl Notifier for NoopNotifier {
    fn request_permission(&self) {}
    fn notify(&self, _request: NotifyRequest) {}
    fn cancel(&self, _id: &str) {}
}

thread_local! {
    static PLATFORM_NOTIFIER: RefCell<Option<NotifierRef>> = const { RefCell::new(None) };
}

/// Installs a platform notifier, replacing any previously installed one.
pub fn set_platform_notifier(notifier: NotifierRef) {
    PLATFORM_NOTIFIER.with(|cell| *cell.borrow_mut() = Some(notifier));
}

/// Removes any registered platform notifier (tests and teardown).
pub fn clear_platform_notifier() {
    PLATFORM_NOTIFIER.with(|cell| *cell.borrow_mut() = None);
}

pub fn default_notifier() -> NotifierRef {
    PLATFORM_NOTIFIER
        .with(|cell| cell.borrow().clone())
        .unwrap_or_else(|| Rc::new(NoopNotifier))
}

pub fn local_notifier() -> CompositionLocal<NotifierRef> {
    thread_local! {
        static LOCAL_NOTIFIER: RefCell<Option<CompositionLocal<NotifierRef>>> = const { RefCell::new(None) };
    }

    LOCAL_NOTIFIER.with(|cell| {
        let mut local = cell.borrow_mut();
        local
            .get_or_insert_with(|| compositionLocalOfWithPolicy(default_notifier, Rc::ptr_eq))
            .clone()
    })
}

#[allow(non_snake_case)]
#[composable]
pub fn ProvideNotifier(content: impl FnOnce()) {
    let notifier = cranpose_core::remember(default_notifier).with(|state| state.clone());
    let local = local_notifier();
    CompositionLocalProvider(vec![local.provides(notifier)], move || {
        content();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Recorder {
        posted: RefCell<Vec<String>>,
    }
    impl Notifier for Recorder {
        fn request_permission(&self) {}
        fn notify(&self, request: NotifyRequest) {
            self.posted.borrow_mut().push(request.id);
        }
        fn cancel(&self, _id: &str) {}
    }

    #[test]
    fn default_is_noop_then_registered_takes_over() {
        clear_platform_notifier();
        // No panic on the no-op default.
        default_notifier().notify(NotifyRequest::new("a", "t", "b"));
        let rec = Rc::new(Recorder::default());
        set_platform_notifier(rec.clone());
        default_notifier().notify(NotifyRequest::new("done", "t", "b").with_deeplink("doc/1"));
        assert_eq!(rec.posted.borrow().as_slice(), &["done".to_string()]);
        clear_platform_notifier();
    }
}
