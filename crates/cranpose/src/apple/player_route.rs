#![expect(
    unsafe_code,
    reason = "AVAudioSession route change notifications use Objective-C observers"
)]

use std::sync::OnceLock;

use cranpose_services::{AudioFocus, publish_audio_focus};
use objc2::{AllocAnyThread, define_class, msg_send, rc::Retained, runtime::NSObject, sel};
use objc2_avf_audio::{
    AVAudioSessionRouteChangeNotification, AVAudioSessionRouteChangeReason,
    AVAudioSessionRouteChangeReasonKey,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSNumber, NSObjectProtocol};

struct SendObserver(Retained<RouteChangeObserver>);
unsafe impl Send for SendObserver {}
unsafe impl Sync for SendObserver {}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "CranposeVideoRouteChangeObserver"]
    #[ivars = ()]
    struct RouteChangeObserver;

    unsafe impl NSObjectProtocol for RouteChangeObserver {}

    impl RouteChangeObserver {
        #[unsafe(method(cranposeAudioRouteChanged:))]
        fn route_changed(&self, notification: &NSNotification) {
            let Some(info) = notification.userInfo() else {
                return;
            };
            let Some(key) = (unsafe { AVAudioSessionRouteChangeReasonKey }) else {
                return;
            };
            let Some(reason_num) = info
                .objectForKey(key)
                .and_then(|value| value.downcast::<NSNumber>().ok())
            else {
                return;
            };
            let reason = AVAudioSessionRouteChangeReason(reason_num.unsignedLongValue() as usize);
            match reason {
                AVAudioSessionRouteChangeReason::OldDeviceUnavailable => {
                    publish_audio_focus(AudioFocus::Lost);
                }
                AVAudioSessionRouteChangeReason::NewDeviceAvailable
                | AVAudioSessionRouteChangeReason::RouteConfigurationChange => {
                    if let Err(err) = crate::apple::audio_session::hold_for_media(true) {
                        log::warn!("cranpose: iOS audio route recovery failed: {err}");
                    }
                }
                _ => {}
            }
        }
    }
);

pub(super) fn install() {
    static OBSERVER: OnceLock<SendObserver> = OnceLock::new();
    let observer = OBSERVER.get_or_init(|| {
        let this = RouteChangeObserver::alloc().set_ivars(());
        SendObserver(unsafe { msg_send![super(this), init] })
    });
    let Some(name) = (unsafe { AVAudioSessionRouteChangeNotification }) else {
        return;
    };
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            &observer.0,
            sel!(cranposeAudioRouteChanged:),
            Some(name),
            None,
        );
    }
}
