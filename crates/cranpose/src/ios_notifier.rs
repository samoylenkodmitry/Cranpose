//! iOS local notifications via `UNUserNotificationCenter`.
//!
//! Registered as the platform notifier (see
//! [`cranpose_services::set_platform_notifier`]) by the iOS backend. iOS has no
//! ongoing/foreground-service notification, so `ongoing` requests post a normal
//! notification. A delegate is installed so notifications also present while the
//! app is foregrounded (CranScan runs OCR in-app) and so a tapped notification's
//! deep-link payload reaches [`cranpose_services::push_notification_deeplink`].
#![allow(unsafe_code)]

use block2::RcBlock;
use cranpose_services::{
    push_notification_deeplink, set_platform_notifier, Notifier, NotifyRequest,
};
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNNotificationResponse,
    UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

/// Maps a delivered notification's identifier to its deep-link payload, so a
/// tap can recover it without touching the `userInfo` dictionary (whose objc2
/// typing is awkward). Populated on `notify`, drained on tap. In-process only:
/// a cold launch straight from a notification does not repopulate it (that path
/// is already unsupported by the pure-Rust UIKit backend).
fn deeplink_map() -> &'static Mutex<HashMap<String, String>> {
    static MAP: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Installs the iOS notifier as the platform notifier and its presentation /
/// tap delegate.
pub(crate) fn register() {
    set_platform_notifier(Arc::new(IosNotifier));
    install_delegate();
}

struct IosNotifier;

impl Notifier for IosNotifier {
    fn request_permission(&self) {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let options = UNAuthorizationOptions::Alert
            | UNAuthorizationOptions::Sound
            | UNAuthorizationOptions::Badge;
        // The completion handler is required; a denied permission simply drops
        // any posted notification.
        let handler = RcBlock::new(|_granted: Bool, _error: *mut NSError| {});
        center.requestAuthorizationWithOptions_completionHandler(options, &handler);
    }

    fn notify(&self, request: NotifyRequest) {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&request.title));
        content.setBody(&NSString::from_str(&request.body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));
        if let Some(link) = request.deeplink.as_deref() {
            if let Ok(mut map) = deeplink_map().lock() {
                map.insert(request.id.clone(), link.to_owned());
            }
        }

        let id = NSString::from_str(&request.id);
        // A nil trigger delivers immediately.
        let un_request =
            UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
        center.addNotificationRequest_withCompletionHandler(&un_request, None);
    }

    fn cancel(&self, id: &str) {
        if let Ok(mut map) = deeplink_map().lock() {
            map.remove(id);
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let ids = NSArray::from_retained_slice(&[NSString::from_str(id)]);
        center.removePendingNotificationRequestsWithIdentifiers(&ids);
        center.removeDeliveredNotificationsWithIdentifiers(&ids);
    }
}

thread_local! {
    /// Keeps the delegate alive (the center holds it weakly).
    static DELEGATE: RefCell<Option<Retained<NotificationDelegate>>> = const { RefCell::new(None) };
}

fn install_delegate() {
    let delegate = NotificationDelegate::new();
    let center = UNUserNotificationCenter::currentNotificationCenter();
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    DELEGATE.with(|cell| *cell.borrow_mut() = Some(delegate));
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CranposeNotificationDelegate"]
    #[ivars = ()]
    struct NotificationDelegate;

    unsafe impl NSObjectProtocol for NotificationDelegate {}

    unsafe impl UNUserNotificationCenterDelegate for NotificationDelegate {
        // Present notifications even while the app is foregrounded (banner +
        // sound), so a "document ready" note shows during in-app OCR.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            let options = UNNotificationPresentationOptions::Banner
                | UNNotificationPresentationOptions::Sound;
            completion.call((options,));
        }

        // The user tapped a notification: forward its deep-link payload.
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            completion: &block2::DynBlock<dyn Fn()>,
        ) {
            if let Some(link) = deeplink_of(response) {
                push_notification_deeplink(link);
            }
            completion.call(());
        }
    }
);

impl NotificationDelegate {
    fn new() -> Retained<Self> {
        let mtm =
            MainThreadMarker::new().expect("notification delegate is installed on the main thread");
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

fn deeplink_of(response: &UNNotificationResponse) -> Option<String> {
    let identifier = response.notification().request().identifier().to_string();
    deeplink_map().lock().ok()?.remove(&identifier)
}
