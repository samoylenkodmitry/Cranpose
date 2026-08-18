use cranpose::AppLauncher;
use cranpose_services::audio::{
    AudioClip, AudioError, AudioPlayer, PlaybackParams, SoundId, VoiceId,
};
use cranpose_services::haptics::{HapticFeedback, Haptics};
use cranpose_services::image_picker::{ImagePicker, ImagePickerError, ImageSource};
use cranpose_services::network_status::{NetworkMonitor, NetworkStatus};
use cranpose_services::notifier::{Notifier, NotifyRequest};
use cranpose_services::purchases::{Product, PurchaseEvent, Purchases, StorePhase, StoreState};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

static AUDIO_CALLS: AtomicUsize = AtomicUsize::new(0);
static HAPTIC_CALLS: AtomicUsize = AtomicUsize::new(0);
static NOTIFIER_CALLS: AtomicUsize = AtomicUsize::new(0);
static FIRST_STORE_LISTENER_CALLS: AtomicUsize = AtomicUsize::new(0);
static SECOND_STORE_LISTENER_CALLS: AtomicUsize = AtomicUsize::new(0);

struct RegisteredAudio;

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
        true
    }
}

struct RegisteredHaptics;

impl Haptics for RegisteredHaptics {
    fn perform(&self, _feedback: HapticFeedback) {
        HAPTIC_CALLS.fetch_add(1, Ordering::Relaxed);
    }
    fn has_amplitude_control(&self) -> bool {
        true
    }
}

struct RegisteredPurchases;

impl Purchases for RegisteredPurchases {
    fn configure(&self, _product_ids: &[&str]) {}
    fn state(&self) -> StoreState {
        StoreState {
            phase: StorePhase::Ready,
            products: vec![Product {
                id: "registered".to_string(),
                display_price: "free".to_string(),
                title: "registered".to_string(),
                description: "registered".to_string(),
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

struct RegisteredNetwork;

impl NetworkMonitor for RegisteredNetwork {
    fn status(&self) -> NetworkStatus {
        NetworkStatus {
            online: false,
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

struct RegisteredImagePicker;

impl ImagePicker for RegisteredImagePicker {
    fn pick_image(
        &self,
        _source: ImageSource,
    ) -> cranpose_services::file_picker::PickerFuture<Result<Option<Vec<u8>>, ImagePickerError>>
    {
        Box::pin(async { Ok(Some(vec![7])) })
    }
}

fn register_services() {
    AUDIO_CALLS.store(0, Ordering::Relaxed);
    HAPTIC_CALLS.store(0, Ordering::Relaxed);
    NOTIFIER_CALLS.store(0, Ordering::Relaxed);
    cranpose_services::set_platform_audio(std::sync::Arc::new(RegisteredAudio));
    cranpose_services::set_platform_haptics(std::sync::Arc::new(RegisteredHaptics));
    cranpose_services::set_platform_purchases(std::sync::Arc::new(RegisteredPurchases));
    cranpose_services::set_platform_network_monitor(std::sync::Arc::new(RegisteredNetwork));
    cranpose_services::set_platform_notifier(std::sync::Arc::new(RegisteredNotifier));
    cranpose_services::set_platform_image_picker(std::sync::Arc::new(RegisteredImagePicker));
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

            cranpose_services::set_store_listener(|| {
                FIRST_STORE_LISTENER_CALLS.fetch_add(1, Ordering::Relaxed);
            });
            cranpose_services::note_store_news();
            cranpose_services::set_store_listener(|| {
                SECOND_STORE_LISTENER_CALLS.fetch_add(1, Ordering::Relaxed);
            });
            cranpose_services::note_store_news();
            assert_eq!(FIRST_STORE_LISTENER_CALLS.load(Ordering::Relaxed), 1);
            assert_eq!(SECOND_STORE_LISTENER_CALLS.load(Ordering::Relaxed), 1);
            robot.exit().expect("exit robot app");
        })
        .try_run(desktop_app::app::DesktopApp)
        .expect("launch robot app");
    println!("PASS: service registrations survive relaunch");
}
