use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

use cranpose::AppLauncher;
use cranpose_services::{
    audio::{AudioClip, AudioError, AudioPlayer, PlaybackParams, SoundId, VoiceId},
    haptics::{HapticFeedback, Haptics},
    image_picker::{ImagePicker, ImagePickerError, ImageSource},
    network_status::{NetworkMonitor, NetworkStatus},
    notifier::{Notifier, NotifyRequest},
    purchases::{Product, PurchaseEvent, Purchases, StorePhase, StoreState},
};

static AUDIO_CALLS: AtomicUsize = AtomicUsize::new(0);
static HAPTIC_CALLS: AtomicUsize = AtomicUsize::new(0);
static NOTIFIER_CALLS: AtomicUsize = AtomicUsize::new(0);
static REPLACEMENT_NOTIFIER_CALLS: AtomicUsize = AtomicUsize::new(0);
static FIRST_STORE_LISTENER_CALLS: AtomicUsize = AtomicUsize::new(0);
static SECOND_STORE_LISTENER_CALLS: AtomicUsize = AtomicUsize::new(0);

struct RegisteredAudio(bool);

impl AudioPlayer for RegisteredAudio {
    fn load_clip(&self, _clip: AudioClip) -> Result<SoundId, AudioError> {
        AUDIO_CALLS.fetch_add(1, Ordering::Relaxed);
        Ok(SoundId::from_raw(1))
    }
    fn play(&self, _id: SoundId, _params: PlaybackParams) {
        AUDIO_CALLS.fetch_add(1, Ordering::Relaxed);
    }
    fn play_loop(&self, _id: SoundId, _params: PlaybackParams) -> VoiceId {
        AUDIO_CALLS.fetch_add(1, Ordering::Relaxed);
        VoiceId::from_raw(1)
    }
    fn stop(&self, _id: SoundId) {}
    fn stop_voice(&self, _voice: VoiceId) {}
    fn set_master_volume(&self, _volume: f32) {}
    fn is_available(&self) -> bool {
        self.0
    }
}

struct RegisteredHaptics(bool);

impl Haptics for RegisteredHaptics {
    fn perform(&self, _feedback: HapticFeedback) {
        HAPTIC_CALLS.fetch_add(1, Ordering::Relaxed);
    }
    fn has_amplitude_control(&self) -> bool {
        self.0
    }
}

struct RegisteredPurchases(&'static str);

impl Purchases for RegisteredPurchases {
    fn configure(&self, _product_ids: &[&str]) {}
    fn state(&self) -> StoreState {
        StoreState {
            phase: StorePhase::Ready,
            products: vec![Product {
                id: self.0.to_string(),
                display_price: "free".to_string(),
                title: self.0.to_string(),
                description: self.0.to_string(),
            }],
            ..StoreState::default()
        }
    }
    fn purchase(&self, _product_id: &str) {}
    fn restore(&self) {}
    fn take_event(&self) -> Option<PurchaseEvent> {
        None
    }
    fn is_connected(&self) -> bool {
        true
    }
    fn reconnect(&self) {}
}

struct RegisteredNetwork(bool);

impl NetworkMonitor for RegisteredNetwork {
    fn status(&self) -> NetworkStatus {
        NetworkStatus {
            online: self.0,
            metered: true,
        }
    }
    fn is_alive(&self) -> bool {
        true
    }
    fn reconnect(&self) {}
}

struct RecoveringPurchases {
    connected: AtomicBool,
    reconnects: AtomicUsize,
}

impl Purchases for RecoveringPurchases {
    fn configure(&self, _product_ids: &[&str]) {}
    fn state(&self) -> StoreState {
        StoreState {
            phase: if self.connected.load(Ordering::Acquire) {
                StorePhase::Ready
            } else {
                StorePhase::Unavailable
            },
            ..StoreState::default()
        }
    }
    fn purchase(&self, _product_id: &str) {}
    fn restore(&self) {}
    fn take_event(&self) -> Option<PurchaseEvent> {
        None
    }
    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }
    fn reconnect(&self) {
        self.reconnects.fetch_add(1, Ordering::AcqRel);
        self.connected.store(true, Ordering::Release);
    }
}

struct RecoveringNetwork {
    online: AtomicBool,
    reconnects: AtomicUsize,
}

impl NetworkMonitor for RecoveringNetwork {
    fn status(&self) -> NetworkStatus {
        NetworkStatus {
            online: self.online.load(Ordering::Acquire),
            metered: false,
        }
    }
    fn is_alive(&self) -> bool {
        self.online.load(Ordering::Acquire)
    }
    fn reconnect(&self) {
        self.reconnects.fetch_add(1, Ordering::AcqRel);
        self.online.store(true, Ordering::Release);
    }
}

struct RegisteredNotifier;

impl Notifier for RegisteredNotifier {
    fn request_permission(&self) {}
    fn notify(&self, _request: NotifyRequest) {
        NOTIFIER_CALLS.fetch_add(1, Ordering::Relaxed);
    }
    fn cancel(&self, _id: &str) {}
}

struct ReplacementNotifier;

impl Notifier for ReplacementNotifier {
    fn request_permission(&self) {}
    fn notify(&self, _request: NotifyRequest) {
        REPLACEMENT_NOTIFIER_CALLS.fetch_add(1, Ordering::Relaxed);
    }
    fn cancel(&self, _id: &str) {}
}

struct RegisteredImagePicker(u8);

impl ImagePicker for RegisteredImagePicker {
    fn pick_image(
        &self,
        _source: ImageSource,
    ) -> cranpose_services::file_picker::PickerFuture<Result<Option<Vec<u8>>, ImagePickerError>>
    {
        let byte = self.0;
        Box::pin(async move { Ok(Some(vec![byte])) })
    }
}

fn register_services() {
    AUDIO_CALLS.store(0, Ordering::Relaxed);
    HAPTIC_CALLS.store(0, Ordering::Relaxed);
    NOTIFIER_CALLS.store(0, Ordering::Relaxed);
    REPLACEMENT_NOTIFIER_CALLS.store(0, Ordering::Relaxed);
    cranpose_services::set_platform_audio(std::sync::Arc::new(RegisteredAudio(true)));
    cranpose_services::set_platform_haptics(std::sync::Arc::new(RegisteredHaptics(true)));
    cranpose_services::set_platform_purchases(std::sync::Arc::new(RegisteredPurchases(
        "registered",
    )));
    cranpose_services::set_platform_network_monitor(std::sync::Arc::new(RegisteredNetwork(false)));
    cranpose_services::set_platform_notifier(std::sync::Arc::new(RegisteredNotifier));
    cranpose_services::set_platform_image_picker(std::sync::Arc::new(RegisteredImagePicker(7)));
}

fn main() {
    AppLauncher::new()
        .with_title("services_registration")
        .with_size(400, 300)
        .with_fonts(desktop_app::fonts::DEMO_FONTS)
        .with_headless(std::env::var("CRANPOSE_HEADLESS").as_deref() == Ok("1"))
        .with_test_driver(|robot| {
            let _ = robot.wait_for_idle();
            register_services();
            assert!(cranpose_services::default_audio().is_available());
            assert!(cranpose_services::default_haptics().has_amplitude_control());
            assert_eq!(cranpose_services::store_state().phase, StorePhase::Ready);
            assert!(!cranpose_services::network_status().online);
            cranpose_services::default_notifier().notify(NotifyRequest::new("main", "", ""));
            assert_eq!(
                pollster::block_on(
                    cranpose_services::default_image_picker().pick_image(ImageSource::Camera)
                )
                .expect("registered image picker on app thread"),
                Some(vec![7])
            );
            let join = std::thread::spawn(|| {
                assert!(cranpose_services::default_audio().is_available());
                assert!(cranpose_services::default_haptics().has_amplitude_control());
                assert_eq!(cranpose_services::store_state().phase, StorePhase::Ready);
                assert!(!cranpose_services::network_status().online);
                cranpose_services::default_notifier().notify(NotifyRequest::new("thread", "", ""));
                assert_eq!(
                    pollster::block_on(
                        cranpose_services::default_image_picker().pick_image(ImageSource::Camera)
                    )
                    .expect("registered image picker after relaunch"),
                    Some(vec![7])
                );
            });
            join.join().expect("service reads survive relaunch");
            assert_eq!(AUDIO_CALLS.load(Ordering::Relaxed), 0);
            assert_eq!(HAPTIC_CALLS.load(Ordering::Relaxed), 0);
            assert_eq!(NOTIFIER_CALLS.load(Ordering::Relaxed), 2);

            let purchases = Arc::new(RecoveringPurchases {
                connected: AtomicBool::new(false),
                reconnects: AtomicUsize::new(0),
            });
            cranpose_services::set_platform_purchases(purchases.clone());
            assert_eq!(cranpose_services::store_state().phase, StorePhase::Ready);
            assert_eq!(purchases.reconnects.load(Ordering::Acquire), 1);

            let network = Arc::new(RecoveringNetwork {
                online: AtomicBool::new(false),
                reconnects: AtomicUsize::new(0),
            });
            cranpose_services::set_platform_network_monitor(network.clone());
            assert!(cranpose_services::network_status().online);
            assert_eq!(network.reconnects.load(Ordering::Acquire), 1);

            let first_store_observer = cranpose_services::observe_store_news(|| {
                FIRST_STORE_LISTENER_CALLS.fetch_add(1, Ordering::Relaxed);
            });
            cranpose_services::note_store_news();
            drop(first_store_observer);
            let _second_store_observer = cranpose_services::observe_store_news(|| {
                SECOND_STORE_LISTENER_CALLS.fetch_add(1, Ordering::Relaxed);
            });
            cranpose_services::note_store_news();
            assert_eq!(FIRST_STORE_LISTENER_CALLS.load(Ordering::Relaxed), 1);
            assert_eq!(SECOND_STORE_LISTENER_CALLS.load(Ordering::Relaxed), 1);

            let cached_audio = cranpose_services::default_audio();
            let cached_haptics = cranpose_services::default_haptics();
            let cached_purchases = cranpose_services::purchases();
            let cached_network = cranpose_services::network_monitor();
            let cached_notifier = cranpose_services::default_notifier();
            let cached_image_picker = cranpose_services::default_image_picker();
            cranpose_services::set_platform_audio(Arc::new(RegisteredAudio(false)));
            cranpose_services::set_platform_haptics(Arc::new(RegisteredHaptics(false)));
            cranpose_services::set_platform_purchases(Arc::new(RegisteredPurchases("replacement")));
            cranpose_services::set_platform_network_monitor(Arc::new(RegisteredNetwork(true)));
            cranpose_services::set_platform_notifier(Arc::new(ReplacementNotifier));
            cranpose_services::set_platform_image_picker(Arc::new(RegisteredImagePicker(9)));
            assert!(!cached_audio.is_available());
            assert!(!cached_haptics.has_amplitude_control());
            assert_eq!(cached_purchases.state().products[0].id, "replacement");
            assert!(cached_network.status().online);
            cached_notifier.notify(NotifyRequest::new("replacement", "", ""));
            assert_eq!(REPLACEMENT_NOTIFIER_CALLS.load(Ordering::Relaxed), 1);
            assert_eq!(
                pollster::block_on(cached_image_picker.pick_image(ImageSource::Camera))
                    .expect("replacement image picker"),
                Some(vec![9])
            );
            robot.exit().expect("exit robot app");
        })
        .try_run(desktop_app::app::DesktopApp)
        .expect("launch robot app");
    println!("PASS: service registrations survive relaunch");
}
