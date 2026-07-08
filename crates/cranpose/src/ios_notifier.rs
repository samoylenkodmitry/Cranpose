//! iOS local notifications via `UNUserNotificationCenter`.
//!
//! Registered as the platform notifier (see
//! [`cranpose_services::set_platform_notifier`]) by the iOS backend. iOS has no
//! ongoing/foreground-service notification, so `ongoing` requests post a normal
//! notification. The tap → deep-link payload is wired separately by the app's
//! deep-link handling.

use block2::RcBlock;
use cranpose_services::{set_platform_notifier, Notifier, NotifyRequest};
use objc2::runtime::Bool;
use objc2_foundation::{NSArray, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
    UNNotificationSound, UNUserNotificationCenter,
};
use std::rc::Rc;

/// Installs the iOS notifier as the platform notifier.
pub(crate) fn register() {
    set_platform_notifier(Rc::new(IosNotifier));
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

        let id = NSString::from_str(&request.id);
        // A nil trigger delivers immediately.
        let un_request =
            UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
        center.addNotificationRequest_withCompletionHandler(&un_request, None);
    }

    fn cancel(&self, id: &str) {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let ids = NSArray::from_retained_slice(&[NSString::from_str(id)]);
        center.removePendingNotificationRequestsWithIdentifiers(&ids);
        center.removeDeliveredNotificationsWithIdentifiers(&ids);
    }
}
