//! Vibrations of the controllers.

use std::{
    mem,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use cranpose_core::CompositionLocal;

use crate::{Hand, InputSource, locals::with_locals};

/// A vibration of a controller: how strong, how fast and for how long.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vibration {
    amplitude: f32,
    frequency: Option<f32>,
    duration: Duration,
}

impl Vibration {
    /// A vibration of `amplitude` from 0 to 1 that lasts `duration`, at the frequency that the
    /// controller picks. An amplitude outside 0 to 1 becomes the nearest end.
    pub fn new(amplitude: f32, duration: Duration) -> Self {
        Self {
            amplitude: if amplitude.is_nan() {
                0.0
            } else {
                amplitude.clamp(0.0, 1.0)
            },
            frequency: None,
            duration,
        }
    }

    /// The vibration at `hertz`. A controller that cannot vibrate at that frequency picks its
    /// own. A value that is not positive leaves the choice to the controller.
    #[must_use]
    pub fn with_frequency(self, hertz: f32) -> Self {
        Self {
            frequency: (hertz.is_finite() && hertz > 0.0).then_some(hertz),
            ..self
        }
    }

    /// The strength, from 0 to 1.
    pub const fn amplitude(&self) -> f32 {
        self.amplitude
    }

    /// The frequency in hertz, or `None` while the controller picks it.
    pub const fn frequency(&self) -> Option<f32> {
        self.frequency
    }

    /// How long the vibration lasts.
    pub const fn duration(&self) -> Duration {
        self.duration
    }
}

// What the app asked of a hand's controller.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum HapticCommand {
    Vibrate(Vibration),
    Stop,
}

// The asks of `asks` without those for a side that a tracked hand points for. That hand holds no
// motor, so its ask has nothing to reach.
#[cfg_attr(
    not(any(test, headset)),
    expect(dead_code, reason = "the headset backend calls it")
)]
pub(crate) fn controller_asks(
    mut asks: [Option<HapticCommand>; 2],
    pointers: [Option<InputSource>; 2],
) -> [Option<HapticCommand>; 2] {
    for (ask, pointer) in asks.iter_mut().zip(pointers) {
        if pointer == Some(InputSource::Hands) {
            *ask = None;
        }
    }
    asks
}

/// The controllers' vibration motors. An app reads the handle from [`local_haptics`], and uses it
/// from the composition, from an event handler or from any other thread: a streaming app plays
/// the vibrations that its host sends as they arrive.
///
/// A vibration reaches the controller in the next frame. A newer one replaces the one that still
/// runs on the same hand, as in OpenXR's `xrApplyHapticFeedback`. The runtime drops a request that
/// it rejects, such as one made while the session has no input focus. A side that a tracked hand
/// points for, with no controller in it, has no motor: the backend drops the ask.
#[derive(Clone)]
pub struct Haptics {
    pending: Arc<Mutex<[Option<HapticCommand>; 2]>>,
}

impl PartialEq for Haptics {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.pending, &other.pending)
    }
}

impl Haptics {
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Haptics that nothing has asked for.
    pub(crate) fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new([None; 2])),
        }
    }

    /// Plays `vibration` on `hand`'s controller.
    pub fn vibrate(&self, hand: Hand, vibration: Vibration) {
        self.ask(hand, HapticCommand::Vibrate(vibration));
    }

    /// Stops the vibration that runs on `hand`'s controller.
    pub fn stop(&self, hand: Hand) {
        self.ask(hand, HapticCommand::Stop);
    }

    fn ask(&self, hand: Hand, command: HapticCommand) {
        // The data is plain, so a panic of another thread leaves it whole
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        pending[hand.index()] = Some(command);
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The asks since the last call, for the left hand and the right hand. A call empties them.
    pub(crate) fn take(&self) -> [Option<HapticCommand>; 2] {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        mem::take(&mut *pending)
    }
}

/// The composition local that holds the controllers' [`Haptics`], or `None` outside a headset.
///
/// ```ignore
/// let haptics = local_haptics().current();
/// // in a click handler:
/// if let Some(haptics) = &haptics {
///     haptics.vibrate(Hand::Right, Vibration::new(0.5, Duration::from_millis(40)));
/// }
/// ```
pub fn local_haptics() -> CompositionLocal<Option<Haptics>> {
    with_locals(|locals| locals.haptics.clone())
}

#[cfg(test)]
#[path = "tests/haptics_tests.rs"]
mod tests;
