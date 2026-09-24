use parking_lot::Mutex;

use super::*;
use crate::run_test_composition;

#[derive(Default)]
struct Rec {
    events: Mutex<Vec<HapticFeedback>>,
}

impl Haptics for Rec {
    fn perform(&self, feedback: HapticFeedback) {
        self.events.lock().push(feedback);
    }
}

#[test]
fn registered_haptics_receives_events() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_haptics();
    default_haptics().perform(HapticFeedback::Selection);

    struct Counter(Arc<Mutex<u32>>);
    impl Haptics for Counter {
        fn perform(&self, _f: HapticFeedback) {
            *self.0.lock() += 1;
        }
    }
    let count = Arc::new(Mutex::new(0));
    set_platform_haptics(Arc::new(Counter(count.clone())));
    default_haptics().perform(HapticFeedback::ImpactMedium);
    assert_eq!(*count.lock(), 1);
    clear_platform_haptics();
}

#[test]
fn noop_backend_answers_every_method_without_panicking() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_haptics();
    let haptics = default_haptics();
    haptics.perform(HapticFeedback::Error);
    haptics.vibrate(30, 128);
    haptics.perform_effect(HapticEffect::DoubleClick);
    haptics.play_pattern(&HapticPattern::new(&[0, 20, 10, 20], &[0, 255, 0, 120]).unwrap());
    haptics.cancel();
    assert!(!haptics.has_amplitude_control());
}

#[test]
fn waveform_rejects_length_mismatch_instead_of_panicking() {
    assert_eq!(
        HapticPattern::new(&[0, 20, 10], &[0, 255]),
        Err(HapticError::LengthMismatch {
            timings: 3,
            amplitudes: 2
        })
    );
    assert_eq!(
        HapticPattern::repeating(&[0, 20], &[0, 255, 128], 0),
        Err(HapticError::LengthMismatch {
            timings: 2,
            amplitudes: 3
        })
    );
}

#[test]
fn waveform_rejects_empty_zero_and_out_of_range_repeat() {
    assert_eq!(HapticPattern::new(&[], &[]), Err(HapticError::Empty));
    assert_eq!(
        HapticPattern::new(&[0, 0, 0], &[0, 255, 0]),
        Err(HapticError::ZeroDuration)
    );
    assert_eq!(
        HapticPattern::repeating(&[0, 20], &[0, 255], 2),
        Err(HapticError::RepeatOutOfRange { index: 2, len: 2 })
    );
    let long = vec![1u32; HapticPattern::MAX_STEPS + 1];
    let amps = vec![1u8; HapticPattern::MAX_STEPS + 1];
    assert_eq!(
        HapticPattern::new(&long, &amps),
        Err(HapticError::TooManySteps {
            len: HapticPattern::MAX_STEPS + 1,
            max: HapticPattern::MAX_STEPS
        })
    );
}

#[test]
fn waveform_exposes_its_shape() {
    let pattern =
        HapticPattern::repeating(&[0, 40, 30, 40], &[0, 200, 0, 90], 1).expect("valid waveform");
    assert_eq!(pattern.timings_ms(), &[0, 40, 30, 40]);
    assert_eq!(pattern.amplitudes(), &[0, 200, 0, 90]);
    assert_eq!(pattern.repeat(), Some(1));
    assert_eq!(pattern.len(), 4);
    assert!(!pattern.is_empty());
    assert_eq!(pattern.total_duration_ms(), 110);
    assert_eq!(pattern.peak_amplitude(), 200);
    assert_eq!(pattern.closest_feedback(), HapticFeedback::ImpactHeavy);

    let light = HapticPattern::new(&[0, 8], &[0, 40]).expect("valid waveform");
    assert_eq!(light.closest_feedback(), HapticFeedback::ImpactLight);
    let medium = HapticPattern::new(&[0, 50], &[0, 120]).expect("valid waveform");
    assert_eq!(medium.closest_feedback(), HapticFeedback::ImpactMedium);
    assert_eq!(light.repeat(), None);
}

#[test]
fn total_duration_saturates_instead_of_overflowing() {
    let pattern = HapticPattern::new(&[u32::MAX, u32::MAX], &[255, 255]).expect("valid");
    assert_eq!(pattern.total_duration_ms(), u32::MAX);
}

#[test]
fn defaulted_methods_fall_back_to_perform() {
    let backend = Arc::new(Rec::default());
    let haptics: HapticsRef = backend.clone();

    haptics.vibrate(10, 20);
    haptics.vibrate(60, 20);
    haptics.vibrate(10, 220);
    haptics.perform_effect(HapticEffect::Tick);
    haptics.perform_effect(HapticEffect::Click);
    haptics.perform_effect(HapticEffect::DoubleClick);
    haptics.perform_effect(HapticEffect::HeavyClick);
    haptics.play_pattern(&HapticPattern::new(&[0, 200], &[0, 255]).unwrap());
    haptics.cancel();

    assert_eq!(
        *backend.events.lock(),
        vec![
            HapticFeedback::ImpactLight,
            HapticFeedback::ImpactMedium,
            HapticFeedback::ImpactHeavy,
            HapticFeedback::Selection,
            HapticFeedback::ImpactLight,
            HapticFeedback::ImpactMedium,
            HapticFeedback::ImpactHeavy,
            HapticFeedback::ImpactHeavy,
        ]
    );
}

#[test]
fn provide_haptics_publishes_the_platform_backend() {
    let _guard = crate::registry::test_service_guard();
    clear_platform_haptics();
    let backend = Arc::new(Rec::default());
    let haptics: HapticsRef = backend.clone();
    set_platform_haptics(haptics);

    run_test_composition(move || {
        ProvideHaptics(|| {
            local_haptics().current().perform(HapticFeedback::Success);
        });
    });

    assert_eq!(*backend.events.lock(), vec![HapticFeedback::Success]);
    clear_platform_haptics();
}
