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

    let ongoing = plain.ongoing(true);
    assert!(ongoing.ongoing);
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
    default_notifier().notify(NotifyRequest::new("a", "t", "b"));
    let rec = Arc::new(Recorder::default());
    set_platform_notifier(rec.clone());
    default_notifier().notify(NotifyRequest::new("done", "t", "b").with_deeplink("doc/1"));
    assert_eq!(rec.posted.lock().as_slice(), &["done".to_string()]);
    clear_platform_notifier();
}
