#![expect(unsafe_code)]

//! The wearer's heart rate on watchOS, through HealthKit.
//!
//! A watch measures the pulse every few seconds only during a workout, so
//! reading starts a workout session and stopping ends it. The session's
//! workout is discarded, so nothing is saved to Health. Starting a session
//! takes permission to save workouts as well as to read the heart rate;
//! `cranpose_capabilities::Use::heart_rate` writes both sentences and the
//! workout background mode into the Info.plist, and the App ID needs the
//! HealthKit capability.
//!
//! A running workout keeps a watch app alive off screen, so the workout ends
//! when the app leaves the screen and starts again when it comes back, for as
//! long as something reads.
//!
//! Every HealthKit object here lives on the main thread.

use std::{
    cell::{Cell, OnceCell, RefCell},
    sync::{Arc, OnceLock},
    time::Duration,
};

use block2::RcBlock;
use cranpose_services::{
    HeartRate, HeartRateError, HeartRateMonitor, HeartRatePermission, HeartRateStatus,
    LifecycleObserver, LifecycleState, current_lifecycle_state, observe_lifecycle,
    publish_heart_rate, publish_heart_rate_permission, set_platform_heart_rate,
};
use dispatch2::{DispatchQueue, run_on_main};
use objc2::{AllocAnyThread, rc::Retained, runtime::Bool};
use objc2_foundation::{NSBundle, NSDate, NSError, NSSet, ns_string};
use objc2_health_kit::{
    HKAuthorizationStatus, HKHealthStore, HKLiveWorkoutBuilder, HKLiveWorkoutDataSource,
    HKObjectType, HKQuantityType, HKQuantityTypeIdentifierHeartRate, HKUnit, HKWorkoutActivityType,
    HKWorkoutConfiguration, HKWorkoutSession, HKWorkoutSessionLocationType,
};

use crate::apple_mobile::on_main_after;

/// How often the reading is taken from the workout. The watch measures about
/// every five seconds during one; a second keeps the reading at most a second
/// behind the watch.
const POLL: Duration = Duration::from_secs(1);

pub(crate) fn register() {
    static FOLLOWS_THE_APP: OnceLock<LifecycleObserver> = OnceLock::new();
    set_platform_heart_rate(Arc::new(WatchHeartRate));
    FOLLOWS_THE_APP
        .get_or_init(|| observe_lifecycle(|event| run_on_main(move |_| follow_the_app(event.to))));
}

struct WatchHeartRate;

/// A running workout and the turn of polling that belongs to it.
struct Workout {
    session: Retained<HKWorkoutSession>,
    builder: Retained<HKLiveWorkoutBuilder>,
    turn: u64,
}

pub(crate) struct HeartRateThreadState {
    store: OnceCell<Retained<HKHealthStore>>,
    workout: RefCell<Option<Workout>>,
    turn: Cell<u64>,
    /// Whether something reads: between `start` and `stop`, whether or not
    /// the app is on screen.
    reading: Cell<bool>,
}

impl HeartRateThreadState {
    pub(crate) const fn new() -> Self {
        Self {
            store: OnceCell::new(),
            workout: RefCell::new(None),
            turn: Cell::new(0),
            reading: Cell::new(false),
        }
    }
}

fn heart_rate_state<R>(f: impl FnOnce(&HeartRateThreadState) -> R) -> R {
    crate::thread_state::with(|state| f(&state.heart_rate))
}

fn with_store<R>(f: impl FnOnce(&HKHealthStore) -> R) -> R {
    heart_rate_state(|state| {
        // SAFETY: creating the store has no preconditions.
        f(state.store.get_or_init(|| unsafe { HKHealthStore::new() }))
    })
}

impl HeartRateMonitor for WatchHeartRate {
    /// A watch with the sensor, in a build that declared the service.
    fn available(&self) -> bool {
        // SAFETY: a class method without arguments.
        let health = unsafe { HKHealthStore::isHealthDataAvailable() };
        health
            && NSBundle::mainBundle()
                .objectForInfoDictionaryKey(ns_string!("NSHealthShareUsageDescription"))
                .is_some()
    }

    /// The answer about saving workouts. HealthKit never tells an app whether
    /// it may read a type, so a person who allowed workouts and not the heart
    /// rate counts as granted and sees no reading.
    fn permission(&self) -> HeartRatePermission {
        let status = run_on_main(|_| {
            with_store(|store| {
                // SAFETY: the store and the workout type are valid objects.
                unsafe { store.authorizationStatusForType(&HKObjectType::workoutType()) }
            })
        });
        match status {
            HKAuthorizationStatus::SharingAuthorized => HeartRatePermission::Granted,
            HKAuthorizationStatus::SharingDenied => HeartRatePermission::Denied,
            _ => HeartRatePermission::NotAsked,
        }
    }

    fn request_permission(&self) {
        run_on_main(|_| {
            // SAFETY: a class method without arguments.
            let workouts = unsafe { HKObjectType::workoutType() };
            let share = NSSet::from_retained_slice(&[workouts.into_super()]);
            let read = NSSet::from_retained_slice(&[heart_rate_type().into_super().into_super()]);
            // HealthKit answers on a queue of its own, and says only whether
            // the request went through; `permission` reads the answer.
            let answered = RcBlock::new(|_done: Bool, _error: *mut NSError| {
                DispatchQueue::main().exec_async(|| publish_heart_rate_permission(true));
            });
            with_store(|store| {
                // SAFETY: the sets and the block outlive the call; HealthKit
                // copies the block until the person answers.
                unsafe {
                    store.requestAuthorizationToShareTypes_readTypes_completion(
                        Some(&share),
                        Some(&read),
                        &answered,
                    );
                }
            });
        });
    }

    fn start(&self) -> Result<(), HeartRateError> {
        run_on_main(|_| {
            heart_rate_state(|state| state.reading.set(true));
            // Off screen it waits for the app to come back.
            if matches!(
                current_lifecycle_state(),
                LifecycleState::Paused | LifecycleState::Stopped
            ) {
                return Ok(());
            }
            start_workout()
        })
        .map_err(HeartRateError::Failed)
    }

    fn stop(&self) {
        run_on_main(|_| {
            heart_rate_state(|state| state.reading.set(false));
            end_workout();
        });
    }
}

/// Ends the workout when the app leaves the screen, and starts it again when
/// the app comes back while something still reads.
fn follow_the_app(state: LifecycleState) {
    match state {
        LifecycleState::Resumed if heart_rate_state(|heart| heart.reading.get()) => {
            if let Err(error) = start_workout() {
                log::warn!("cranpose: the heart-rate workout did not start again: {error}");
            }
        }
        LifecycleState::Paused | LifecycleState::Stopped
            if heart_rate_state(|heart| heart.workout.borrow().is_some()) =>
        {
            end_workout();
            publish_heart_rate(HeartRate {
                status: HeartRateStatus::Acquiring,
                bpm: None,
            });
        }
        _ => {}
    }
}

fn end_workout() {
    let Some(workout) = heart_rate_state(|state| state.workout.take()) else {
        return;
    };
    // SAFETY: ending the session and dropping its workout on the thread that
    // started them.
    unsafe {
        workout.session.end();
        workout.builder.discardWorkout();
    }
}

fn start_workout() -> Result<(), String> {
    if heart_rate_state(|state| state.workout.borrow().is_some()) {
        return Ok(());
    }
    with_store(|store| {
        // SAFETY: a new configuration, set before any session reads it.
        let configuration = unsafe {
            let configuration = HKWorkoutConfiguration::new();
            configuration.setActivityType(HKWorkoutActivityType::Other);
            configuration.setLocationType(HKWorkoutSessionLocationType::Indoor);
            configuration
        };
        // SAFETY: the store and the configuration are valid objects.
        let session = unsafe {
            HKWorkoutSession::initWithHealthStore_configuration_error(
                HKWorkoutSession::alloc(),
                store,
                &configuration,
            )
        }
        .map_err(|error| format!("no workout session: {error}"))?;
        // SAFETY: the builder and its data source belong to this session and
        // store, and collect from the moment the session starts.
        let builder = unsafe {
            let builder = session.associatedWorkoutBuilder();
            let source = HKLiveWorkoutDataSource::initWithHealthStore_workoutConfiguration(
                HKLiveWorkoutDataSource::alloc(),
                store,
                Some(&configuration),
            );
            builder.setDataSource(Some(&source));
            let now = NSDate::now();
            session.startActivityWithDate(Some(&now));
            let began = RcBlock::new(|done: Bool, _error: *mut NSError| {
                if !done.as_bool() {
                    log::warn!("cranpose: the workout did not start collecting the heart rate");
                }
            });
            builder.beginCollectionWithStartDate_completion(&now, &began);
            builder
        };
        let turn = heart_rate_state(|state| {
            let turn = state.turn.get().wrapping_add(1);
            state.turn.set(turn);
            *state.workout.borrow_mut() = Some(Workout {
                session,
                builder,
                turn,
            });
            turn
        });
        poll(turn);
        Ok(())
    })
}

/// Publishes the workout's latest heart rate every [`POLL`] for as long as
/// the workout of `turn` runs.
fn poll(turn: u64) {
    on_main_after(POLL, move |_| {
        let reading = heart_rate_state(|state| {
            state
                .workout
                .borrow()
                .as_ref()
                .filter(|workout| workout.turn == turn)
                .map(|workout| latest(&workout.builder))
        });
        if let Some(reading) = reading {
            publish_heart_rate(reading);
            poll(turn);
        }
    });
}

fn latest(builder: &HKLiveWorkoutBuilder) -> HeartRate {
    // SAFETY: the builder's statistics and their quantity are valid objects,
    // read on the builder's thread.
    let bpm = unsafe {
        builder
            .statisticsForType(&heart_rate_type())
            .and_then(|statistics| statistics.mostRecentQuantity())
            .map(|quantity| quantity.doubleValueForUnit(&beats_per_minute()))
    };
    match bpm {
        Some(bpm) if bpm.is_finite() && bpm > 0.0 => HeartRate {
            status: HeartRateStatus::Live,
            bpm: Some(bpm as f32),
        },
        _ => HeartRate {
            status: HeartRateStatus::Acquiring,
            bpm: None,
        },
    }
}

fn heart_rate_type() -> Retained<HKQuantityType> {
    // SAFETY: the identifier is HealthKit's own constant, and HealthKit
    // knows the type on every watch.
    unsafe { HKObjectType::quantityTypeForIdentifier(HKQuantityTypeIdentifierHeartRate) }
        .expect("HealthKit has a heart-rate type")
}

fn beats_per_minute() -> Retained<HKUnit> {
    // SAFETY: two of HealthKit's own units.
    unsafe { HKUnit::countUnit().unitDividedByUnit(&HKUnit::minuteUnit()) }
}
