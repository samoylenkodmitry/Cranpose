//! Haptic feedback — the framework analogue of Jetpack's `LocalHapticFeedback`.
//!
//! The compiled-in default is a no-op; platform backends install a real
//! implementation through [`set_platform_haptics`] (iOS `UIFeedbackGenerator`,
//! Android `Vibrator`). Desktop and the web have no haptics and drop it.
//!
//! Two layers sit on one trait. [`HapticFeedback`] names the seven semantic
//! events every platform can express, and is what UI code should use.
//! [`HapticPattern`], [`Haptics::vibrate`] and [`Haptics::perform_effect`]
//! address the vibrator directly, which is what an app that designs its own
//! set of distinct "feels" needs; every one of them carries a defaulted body
//! that degrades to the closest [`HapticFeedback`] constant, so a backend that
//! implements only [`Haptics::perform`] still answers the whole trait.

use crate::registry::ServiceRegistry;
use cranpose_core::compositionLocalOfWithPolicy;
use cranpose_core::CompositionLocal;
use cranpose_core::CompositionLocalProvider;
use cranpose_macros::composable;
use std::cell::RefCell;
use std::sync::{Arc, OnceLock};

/// A haptic feedback event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HapticFeedback {
    /// A light physical impact (e.g. a small control toggling).
    ImpactLight,
    /// A medium physical impact (e.g. a button press).
    ImpactMedium,
    /// A heavy physical impact (e.g. a large snap).
    ImpactHeavy,
    /// A selection change (e.g. scrubbing through a picker).
    Selection,
    /// A task completed successfully.
    Success,
    /// A warning.
    Warning,
    /// An error / rejected action.
    Error,
}

/// A system-defined vibration primitive.
///
/// These map to Android's `VibrationEffect.EFFECT_*` constants, which are tuned
/// per device by the manufacturer and therefore feel more native than a
/// hand-timed one-shot of the same length.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HapticEffect {
    /// `VibrationEffect.EFFECT_CLICK`.
    Click,
    /// `VibrationEffect.EFFECT_TICK` — the lightest primitive.
    Tick,
    /// `VibrationEffect.EFFECT_DOUBLE_CLICK`.
    DoubleClick,
    /// `VibrationEffect.EFFECT_HEAVY_CLICK`.
    HeavyClick,
}

impl HapticEffect {
    /// The closest [`HapticFeedback`] constant, used by the trait's defaulted
    /// bodies and by backends without predefined effects.
    pub fn closest_feedback(self) -> HapticFeedback {
        match self {
            HapticEffect::Tick => HapticFeedback::Selection,
            HapticEffect::Click => HapticFeedback::ImpactLight,
            HapticEffect::DoubleClick => HapticFeedback::ImpactMedium,
            HapticEffect::HeavyClick => HapticFeedback::ImpactHeavy,
        }
    }
}

/// Why a haptic pattern could not be built.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum HapticError {
    /// Timings and amplitudes must describe the same number of steps.
    #[error("waveform has {timings} timings and {amplitudes} amplitudes; they must match")]
    LengthMismatch {
        /// How many timings were supplied.
        timings: usize,
        /// How many amplitudes were supplied.
        amplitudes: usize,
    },
    /// A waveform needs at least one step.
    #[error("waveform has no steps")]
    Empty,
    /// A waveform whose timings are all zero would never play.
    #[error("waveform has a total duration of zero")]
    ZeroDuration,
    /// The repeat index must point at a step of the waveform.
    #[error("repeat index {index} is out of range for a {len}-step waveform")]
    RepeatOutOfRange {
        /// The requested repeat index.
        index: usize,
        /// How many steps the waveform has.
        len: usize,
    },
    /// The waveform is longer than the platform vibrator accepts.
    #[error("waveform has {len} steps, more than the maximum of {max}")]
    TooManySteps {
        /// How many steps were supplied.
        len: usize,
        /// The maximum step count.
        max: usize,
    },
}

/// A vibration waveform: alternating durations with a target amplitude each.
///
/// This is Android's `VibrationEffect.createWaveform(long[], int[], int)` in
/// framework terms. Index 0 is an off period by convention (its amplitude is
/// usually 0), then on, then off — but nothing enforces that, so an app is
/// free to shape a ramp out of consecutive non-zero amplitudes.
///
/// Amplitudes run 0 (off) to 255 (the device's strongest). Devices without
/// amplitude control treat any non-zero amplitude as full strength; check
/// [`Haptics::has_amplitude_control`] before designing around subtle levels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HapticPattern {
    timings_ms: Vec<u32>,
    amplitudes: Vec<u8>,
    repeat: Option<usize>,
}

impl HapticPattern {
    /// The longest waveform the framework passes to a platform vibrator.
    /// Android's own limit is device-defined and far lower in practice; this
    /// bound keeps a malformed pattern from reaching JNI at all.
    pub const MAX_STEPS: usize = 512;

    /// Builds a one-shot waveform.
    ///
    /// Fails when the two slices differ in length, when there are no steps, or
    /// when every timing is zero.
    pub fn new(timings_ms: &[u32], amplitudes: &[u8]) -> Result<HapticPattern, HapticError> {
        Self::build(timings_ms, amplitudes, None)
    }

    /// Builds a waveform that loops back to `repeat_index` until
    /// [`Haptics::cancel`] stops it.
    pub fn repeating(
        timings_ms: &[u32],
        amplitudes: &[u8],
        repeat_index: usize,
    ) -> Result<HapticPattern, HapticError> {
        Self::build(timings_ms, amplitudes, Some(repeat_index))
    }

    fn build(
        timings_ms: &[u32],
        amplitudes: &[u8],
        repeat: Option<usize>,
    ) -> Result<HapticPattern, HapticError> {
        if timings_ms.len() != amplitudes.len() {
            return Err(HapticError::LengthMismatch {
                timings: timings_ms.len(),
                amplitudes: amplitudes.len(),
            });
        }
        if timings_ms.is_empty() {
            return Err(HapticError::Empty);
        }
        if timings_ms.len() > HapticPattern::MAX_STEPS {
            return Err(HapticError::TooManySteps {
                len: timings_ms.len(),
                max: HapticPattern::MAX_STEPS,
            });
        }
        if timings_ms.iter().all(|step| *step == 0) {
            return Err(HapticError::ZeroDuration);
        }
        if let Some(index) = repeat {
            if index >= timings_ms.len() {
                return Err(HapticError::RepeatOutOfRange {
                    index,
                    len: timings_ms.len(),
                });
            }
        }
        Ok(HapticPattern {
            timings_ms: timings_ms.to_vec(),
            amplitudes: amplitudes.to_vec(),
            repeat,
        })
    }

    /// The per-step durations in milliseconds.
    pub fn timings_ms(&self) -> &[u32] {
        &self.timings_ms
    }

    /// The per-step amplitudes, 0 to 255.
    pub fn amplitudes(&self) -> &[u8] {
        &self.amplitudes
    }

    /// The index the waveform loops back to, if it repeats.
    pub fn repeat(&self) -> Option<usize> {
        self.repeat
    }

    /// How many steps the waveform has.
    pub fn len(&self) -> usize {
        self.timings_ms.len()
    }

    /// Always `false`: a pattern cannot be built with no steps.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// One pass through the waveform, in milliseconds.
    pub fn total_duration_ms(&self) -> u32 {
        self.timings_ms
            .iter()
            .fold(0u32, |sum, step| sum.saturating_add(*step))
    }

    /// The strongest amplitude in the waveform, which is what a backend
    /// without waveform support falls back on.
    pub fn peak_amplitude(&self) -> u8 {
        self.amplitudes.iter().copied().max().unwrap_or(0)
    }

    /// The closest [`HapticFeedback`] constant for this pattern, derived from
    /// its strength and length. Backends without waveform support use it.
    pub fn closest_feedback(&self) -> HapticFeedback {
        let peak = u32::from(self.peak_amplitude());
        let duration = self.total_duration_ms();
        if peak >= 200 || duration >= 120 {
            HapticFeedback::ImpactHeavy
        } else if peak >= 110 || duration >= 40 {
            HapticFeedback::ImpactMedium
        } else {
            HapticFeedback::ImpactLight
        }
    }
}

/// Performs haptic feedback. Installed by the platform backend; the default is
/// a no-op.
///
/// Only [`perform`](Haptics::perform) has to be implemented. Every other method
/// falls back to it, so extending this trait cannot break an existing backend.
pub trait Haptics: Send + Sync {
    /// Plays a semantic feedback event.
    fn perform(&self, feedback: HapticFeedback);

    /// Vibrates once for `duration_ms` at `amplitude` (1 to 255; 0 means the
    /// device default strength).
    ///
    /// Maps to `VibrationEffect.createOneShot(long, int)` on Android. The
    /// defaulted body picks the closest [`HapticFeedback`] constant.
    fn vibrate(&self, duration_ms: u32, amplitude: u8) {
        let feedback = if amplitude >= 200 || duration_ms >= 120 {
            HapticFeedback::ImpactHeavy
        } else if amplitude >= 110 || duration_ms >= 40 {
            HapticFeedback::ImpactMedium
        } else {
            HapticFeedback::ImpactLight
        };
        self.perform(feedback);
    }

    /// Plays a waveform pattern.
    ///
    /// Maps to `VibrationEffect.createWaveform(long[], int[], int)` on Android.
    /// The defaulted body plays [`HapticPattern::closest_feedback`] once.
    fn play_pattern(&self, pattern: &HapticPattern) {
        self.perform(pattern.closest_feedback());
    }

    /// Plays a system-defined primitive.
    ///
    /// Maps to `VibrationEffect.createPredefined(int)` on Android. The
    /// defaulted body plays [`HapticEffect::closest_feedback`].
    fn perform_effect(&self, effect: HapticEffect) {
        self.perform(effect.closest_feedback());
    }

    /// Stops any vibration in progress, including a repeating waveform.
    /// Backends with no way to cancel leave this as a no-op.
    fn cancel(&self) {}

    /// Whether the device reproduces amplitudes rather than treating every
    /// non-zero level as full strength.
    fn has_amplitude_control(&self) -> bool {
        false
    }
}

pub type HapticsRef = Arc<dyn Haptics>;

struct NoopHaptics;

impl Haptics for NoopHaptics {
    fn perform(&self, _feedback: HapticFeedback) {}
}

static PLATFORM_HAPTICS: ServiceRegistry<dyn Haptics> = ServiceRegistry::new();
static NOOP_HAPTICS: OnceLock<HapticsRef> = OnceLock::new();
static DEFAULT_HAPTICS: OnceLock<HapticsRef> = OnceLock::new();

struct PlatformHaptics;

fn registered_haptics() -> HapticsRef {
    PLATFORM_HAPTICS
        .get_or_warn("haptics")
        .unwrap_or_else(|| NOOP_HAPTICS.get_or_init(|| Arc::new(NoopHaptics)).clone())
}

impl Haptics for PlatformHaptics {
    fn perform(&self, feedback: HapticFeedback) {
        registered_haptics().perform(feedback);
    }

    fn vibrate(&self, duration_ms: u32, amplitude: u8) {
        registered_haptics().vibrate(duration_ms, amplitude);
    }

    fn play_pattern(&self, pattern: &HapticPattern) {
        registered_haptics().play_pattern(pattern);
    }

    fn perform_effect(&self, effect: HapticEffect) {
        registered_haptics().perform_effect(effect);
    }

    fn cancel(&self) {
        registered_haptics().cancel();
    }

    fn has_amplitude_control(&self) -> bool {
        registered_haptics().has_amplitude_control()
    }
}

/// Installs a platform haptics implementation, replacing any previous one.
pub fn set_platform_haptics(haptics: HapticsRef) {
    PLATFORM_HAPTICS.set(haptics);
}

/// Removes any registered platform haptics (tests and teardown).
pub fn clear_platform_haptics() {
    PLATFORM_HAPTICS.clear();
}

pub fn default_haptics() -> HapticsRef {
    DEFAULT_HAPTICS
        .get_or_init(|| Arc::new(PlatformHaptics))
        .clone()
}

pub fn local_haptics() -> CompositionLocal<HapticsRef> {
    thread_local! {
        static LOCAL_HAPTICS: RefCell<Option<CompositionLocal<HapticsRef>>> = const { RefCell::new(None) };
    }

    LOCAL_HAPTICS.with(|cell| {
        let mut local = cell.borrow_mut();
        local
            .get_or_insert_with(|| compositionLocalOfWithPolicy(default_haptics, Arc::ptr_eq))
            .clone()
    })
}

#[allow(non_snake_case)]
#[composable]
pub fn ProvideHaptics(content: impl FnOnce()) {
    let haptics = cranpose_core::remember(default_haptics).with(|state| state.clone());
    let local = local_haptics();
    CompositionLocalProvider(vec![local.provides(haptics)], move || {
        content();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_test_composition;
    use parking_lot::Mutex;

    /// A backend that implements only `perform`, exactly as one written before
    /// the waveform methods existed would.
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
        default_haptics().perform(HapticFeedback::Selection); // no-op, no panic

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
        let pattern = HapticPattern::repeating(&[0, 40, 30, 40], &[0, 200, 0, 90], 1)
            .expect("valid waveform");
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
}
