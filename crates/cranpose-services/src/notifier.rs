//! Local notifications: ask permission and post user-visible notifications.
//!
//! The compiled-in default is a no-op; platform backends install a real
//! notifier through [`set_platform_notifier`] (iOS `UNUserNotificationCenter`,
//! Android `NotificationManager`, the web Notifications API). Desktop has a
//! zero-dependency built-in behind the `notifier-native` feature (notify-send
//! / osascript / PowerShell toast); CI without a notification service simply
//! drops them.

use std::{
    cell::RefCell,
    sync::{Arc, OnceLock},
};

use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOfWithPolicy};
use cranpose_macros::composable;
use parking_lot::Mutex;

use crate::registry::ServiceRegistry;

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
pub trait Notifier: Send + Sync {
    /// Request permission to show notifications (idempotent; safe to call more
    /// than once).
    fn request_permission(&self);
    /// Post (or replace, by id) a local notification.
    fn notify(&self, request: NotifyRequest);
    /// Remove a delivered or pending notification by id.
    fn cancel(&self, id: &str);
}

pub type NotifierRef = Arc<dyn Notifier>;

fn pending_deeplink_slot() -> &'static Mutex<Option<String>> {
    static SLOT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Record the deep-link payload of a notification the user just tapped. Called
/// by the platform backend's notification delegate; the app drains it via
/// [`take_notification_deeplink`].
pub fn push_notification_deeplink(link: String) {
    *pending_deeplink_slot().lock() = Some(link);
}

/// Take (and clear) the deep-link payload of the most recently tapped
/// notification, if any. Polled by the app's deep-link handling.
pub fn take_notification_deeplink() -> Option<String> {
    pending_deeplink_slot().lock().take()
}

#[cfg(not(all(
    feature = "notifier-native",
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
)))]
struct NoopNotifier;

#[cfg(not(all(
    feature = "notifier-native",
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
)))]
impl Notifier for NoopNotifier {
    fn request_permission(&self) {}
    fn notify(&self, _request: NotifyRequest) {}
    fn cancel(&self, _id: &str) {}
}

static PLATFORM_NOTIFIER: ServiceRegistry<dyn Notifier> = ServiceRegistry::new();
static DEFAULT_NOTIFIER: OnceLock<NotifierRef> = OnceLock::new();

struct PlatformNotifier;

fn registered_notifier() -> NotifierRef {
    PLATFORM_NOTIFIER
        .get_or_warn("notifier")
        .unwrap_or_else(|| {
            #[cfg(all(
                feature = "notifier-native",
                not(target_arch = "wasm32"),
                not(target_os = "android"),
                not(target_os = "ios")
            ))]
            {
                Arc::new(desktop::DesktopNotifier)
            }
            #[cfg(not(all(
                feature = "notifier-native",
                not(target_arch = "wasm32"),
                not(target_os = "android"),
                not(target_os = "ios")
            )))]
            {
                Arc::new(NoopNotifier)
            }
        })
}

impl Notifier for PlatformNotifier {
    fn request_permission(&self) {
        registered_notifier().request_permission();
    }

    fn notify(&self, request: NotifyRequest) {
        registered_notifier().notify(request);
    }

    fn cancel(&self, id: &str) {
        registered_notifier().cancel(id);
    }
}

pub fn set_platform_notifier(notifier: NotifierRef) {
    PLATFORM_NOTIFIER.set(notifier);
}

pub fn clear_platform_notifier() {
    PLATFORM_NOTIFIER.clear();
}

pub fn default_notifier() -> NotifierRef {
    DEFAULT_NOTIFIER
        .get_or_init(|| Arc::new(PlatformNotifier))
        .clone()
}

/// Zero-dependency desktop notifications through the OS-native CLI surface:
/// `notify-send` (Linux/BSD, libnotify), `osascript` (macOS), and a PowerShell
/// toast (Windows). Replace-by-id and cancel are honored where the tool
/// supports them (Linux); elsewhere they are best-effort. Deep-link taps are
/// not observable through these CLIs, so `NotifyRequest::deeplink` is not
/// delivered on desktop.
#[cfg(all(
    feature = "notifier-native",
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios")
))]
mod desktop {
    use std::process::{Command, Stdio};

    use super::{Notifier, NotifyRequest};

    pub(super) struct DesktopNotifier;

    /// Stable numeric id for notify-send's replace-id from the request's
    /// string id (FNV-1a folded to a positive u32; 0 means "no id" there).
    #[cfg(target_os = "linux")]
    fn numeric_id(id: &str) -> u32 {
        let mut hash: u32 = 0x811c_9dc5;
        for byte in id.as_bytes() {
            hash ^= u32::from(*byte);
            hash = hash.wrapping_mul(0x0100_0193);
        }
        hash.max(1)
    }

    fn spawn_silent(mut command: Command) {
        let _ = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }

    impl Notifier for DesktopNotifier {
        fn request_permission(&self) {}

        #[allow(unused_variables)]
        fn notify(&self, request: NotifyRequest) {
            #[cfg(target_os = "linux")]
            {
                let mut command = Command::new("notify-send");
                command
                    .arg("--replace-id")
                    .arg(numeric_id(&request.id).to_string())
                    .arg("--app-name")
                    .arg("cranpose")
                    .arg(&request.title)
                    .arg(&request.body);
                spawn_silent(command);
            }
            #[cfg(target_os = "macos")]
            {
                let script = format!(
                    "display notification \"{}\" with title \"{}\"",
                    request.body.replace('\\', "\\\\").replace('"', "\\\""),
                    request.title.replace('\\', "\\\\").replace('"', "\\\"")
                );
                let mut command = Command::new("osascript");
                command.arg("-e").arg(script);
                spawn_silent(command);
            }
            #[cfg(target_os = "windows")]
            {
                let script = format!(
                    "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
                     $t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
                     $n = $t.GetElementsByTagName('text'); \
                     $n.Item(0).AppendChild($t.CreateTextNode('{}')) > $null; \
                     $n.Item(1).AppendChild($t.CreateTextNode('{}')) > $null; \
                     [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('cranpose').Show([Windows.UI.Notifications.ToastNotification]::new($t))",
                    request.title.replace('\'', "''"),
                    request.body.replace('\'', "''")
                );
                let mut command = Command::new("powershell");
                command.arg("-NoProfile").arg("-Command").arg(script);
                spawn_silent(command);
            }
        }

        #[allow(unused_variables)]
        fn cancel(&self, id: &str) {
            #[cfg(target_os = "linux")]
            {
                let mut command = Command::new("gdbus");
                command.args([
                    "call",
                    "--session",
                    "--dest",
                    "org.freedesktop.Notifications",
                    "--object-path",
                    "/org/freedesktop/Notifications",
                    "--method",
                    "org.freedesktop.Notifications.CloseNotification",
                ]);
                command.arg(numeric_id(id).to_string());
                spawn_silent(command);
            }
        }
    }
}

pub fn local_notifier() -> CompositionLocal<NotifierRef> {
    thread_local! {
        static LOCAL_NOTIFIER: RefCell<Option<CompositionLocal<NotifierRef>>> = const { RefCell::new(None) };
    }

    LOCAL_NOTIFIER.with(|cell| {
        let mut local = cell.borrow_mut();
        local
            .get_or_insert_with(|| compositionLocalOfWithPolicy(default_notifier, Arc::ptr_eq))
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
    use parking_lot::Mutex;

    use super::*;

    #[test]
    fn an_ongoing_request_is_the_one_a_user_cannot_swipe_away() {
        let plain = NotifyRequest::new("scan", "Recognising", "3 of 12 pages");
        assert!(!plain.ongoing, "a plain notification is dismissable");
        assert_eq!(plain.id, "scan");
        assert_eq!(plain.title, "Recognising");
        assert_eq!(plain.body, "3 of 12 pages");
        assert_eq!(plain.deeplink, None);

        // Progress that is still running says so, which is what stops the
        // platform letting the user swipe away work that is still going.
        let ongoing = plain.clone().ongoing(true);
        assert!(ongoing.ongoing);
        // And a finished one goes back to being dismissable.
        assert!(!ongoing.ongoing(false).ongoing);
    }

    #[derive(Default)]
    struct Recorder {
        posted: Mutex<Vec<String>>,
    }
    impl Notifier for Recorder {
        fn request_permission(&self) {}
        fn notify(&self, request: NotifyRequest) {
            self.posted.lock().push(request.id);
        }
        fn cancel(&self, _id: &str) {}
    }

    #[test]
    fn default_is_noop_then_registered_takes_over() {
        let _guard = crate::registry::test_service_guard();
        clear_platform_notifier();
        // No panic on the no-op default.
        default_notifier().notify(NotifyRequest::new("a", "t", "b"));
        let rec = Arc::new(Recorder::default());
        set_platform_notifier(rec.clone());
        default_notifier().notify(NotifyRequest::new("done", "t", "b").with_deeplink("doc/1"));
        assert_eq!(rec.posted.lock().as_slice(), &["done".to_string()]);
        clear_platform_notifier();
    }
}
