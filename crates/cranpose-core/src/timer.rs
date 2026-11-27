#[cfg(not(any(
    feature = "host-timers",
    target_os = "emscripten",
    target_os = "none",
    target_vendor = "apple",
    all(target_arch = "wasm32", target_os = "unknown")
)))]
use std::sync::{Condvar, Mutex, PoisonError};
#[cfg(not(any(
    feature = "host-timers",
    target_os = "emscripten",
    target_os = "none",
    target_vendor = "apple",
    all(target_arch = "wasm32", target_os = "unknown")
)))]
use std::time::Duration;
use std::{
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    task::Waker,
};

#[cfg(all(
    not(feature = "host-timers"),
    target_arch = "wasm32",
    target_os = "unknown"
))]
use wasm_bindgen::JsCast;
use web_time::Instant;

#[cfg(any(
    feature = "host-timers",
    target_os = "emscripten",
    target_os = "espidf",
    target_os = "none",
    not(any(
        target_vendor = "apple",
        all(target_arch = "wasm32", target_os = "unknown")
    ))
))]
pub(crate) struct Alarm {
    deadline: Instant,
    waker: Waker,
    fired: Arc<AtomicBool>,
}

// Arms the alarm of a `Delay`. Under `host-timers`, Emscripten and bare-metal
// targets, every delay waits in the thread's runtime state so the host drives
// it through UI drains and `next_alarm_deadline`. Under ESP-IDF, delays on a
// composition thread also wait there to avoid a 9 KB timer thread stack, while
// off-UI delays fall back to the timer thread.
pub(crate) fn arm(deadline: Instant, waker: Waker, fired: Arc<AtomicBool>) {
    #[cfg(all(not(feature = "host-timers"), target_os = "espidf"))]
    if crate::runtime::current_runtime_handle().is_some() {
        arm_host_alarm(Alarm {
            deadline,
            waker,
            fired,
        });
        return;
    }
    timer().arm(deadline, waker, fired);
}

struct Timer {
    #[cfg(not(any(
        feature = "host-timers",
        target_os = "emscripten",
        target_os = "none",
        target_vendor = "apple",
        all(target_arch = "wasm32", target_os = "unknown")
    )))]
    alarms: Mutex<Vec<Alarm>>,
    #[cfg(not(any(
        feature = "host-timers",
        target_os = "emscripten",
        target_os = "none",
        target_vendor = "apple",
        all(target_arch = "wasm32", target_os = "unknown")
    )))]
    wake: Condvar,
}

fn timer() -> &'static Timer {
    static TIMER: OnceLock<&'static Timer> = OnceLock::new();
    TIMER.get_or_init(|| {
        let timer: &'static Timer = Box::leak(Box::new(Timer::new()));
        timer.start();
        timer
    })
}

#[cfg(not(any(
    feature = "host-timers",
    target_os = "emscripten",
    target_os = "none",
    target_vendor = "apple",
    all(target_arch = "wasm32", target_os = "unknown")
)))]
impl Timer {
    fn new() -> Self {
        Self {
            alarms: Mutex::new(Vec::new()),
            wake: Condvar::new(),
        }
    }

    fn start(&'static self) {
        std::thread::Builder::new()
            .name("cranpose-timer".to_string())
            .spawn(move || self.run())
            .expect("the timer thread starts");
    }

    fn run(&self) {
        let mut alarms = self.alarms.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            let now = Instant::now();
            let mut due = Vec::new();
            let mut next: Option<Duration> = None;
            alarms.retain(|alarm| {
                if alarm.deadline <= now {
                    due.push((alarm.waker.clone(), Arc::clone(&alarm.fired)));
                    false
                } else {
                    let remaining = alarm.deadline - now;
                    next = Some(next.map_or(remaining, |current| current.min(remaining)));
                    true
                }
            });

            if !due.is_empty() {
                drop(alarms);
                for (waker, fired) in due {
                    fired.store(true, Ordering::Release);
                    waker.wake();
                }
                alarms = self.alarms.lock().unwrap_or_else(PoisonError::into_inner);
                continue;
            }

            alarms = match next {
                Some(timeout) => {
                    self.wake
                        .wait_timeout(alarms, timeout)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
                None => self
                    .wake
                    .wait(alarms)
                    .unwrap_or_else(PoisonError::into_inner),
            };
        }
    }

    fn arm(&self, deadline: Instant, waker: Waker, fired: Arc<AtomicBool>) {
        let mut alarms = self.alarms.lock().unwrap_or_else(PoisonError::into_inner);
        alarms.push(Alarm {
            deadline,
            waker,
            fired,
        });
        self.wake.notify_one();
    }
}

// Apple platforms hand each deadline to Grand Central Dispatch on the
// user-interactive queue. A thread sleeping on a condition variable at default
// QoS wakes milliseconds late on macOS due to timer coalescing.
#[cfg(all(not(feature = "host-timers"), target_vendor = "apple"))]
impl Timer {
    fn new() -> Self {
        Self {}
    }

    fn start(&'static self) {}

    fn arm(&self, deadline: Instant, waker: Waker, fired: Arc<AtomicBool>) {
        use dispatch2::{DispatchQoS, DispatchQueue, DispatchTime, GlobalQueueIdentifier};

        let nanos = deadline
            .saturating_duration_since(Instant::now())
            .as_nanos()
            .min(i64::MAX as u128) as i64;
        let queue = DispatchQueue::global_queue(GlobalQueueIdentifier::QualityOfService(
            DispatchQoS::UserInteractive,
        ));
        let fire = move || {
            fired.store(true, Ordering::Release);
            waker.wake();
        };
        if queue.after(DispatchTime::NOW.time(nanos), fire).is_err() {
            log::error!("cranpose: GCD refused a timer; the delay never resolves");
        }
    }
}

#[cfg(all(
    not(feature = "host-timers"),
    target_arch = "wasm32",
    target_os = "unknown"
))]
impl Timer {
    fn new() -> Self {
        Self {}
    }

    fn start(&'static self) {}

    fn arm(&self, deadline: Instant, waker: Waker, fired: Arc<AtomicBool>) {
        let millis = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(i32::MAX as u128) as i32;
        let callback = wasm_bindgen::closure::Closure::once_into_js(move || {
            fired.store(true, Ordering::Release);
            waker.wake();
        });
        let scheduled = web_sys::window().and_then(|window| {
            window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    callback.unchecked_ref(),
                    millis,
                )
                .ok()
        });
        if scheduled.is_none() {
            log::warn!("cranpose: no window timer is available; the delay resolves immediately");
        }
    }
}

// Host-driven timers keep alarms in the thread's runtime state, which the
// runtime drains as the host drives frames (Emscripten, bare-metal OS hosts,
// and single-threaded embeddings that enable `host-timers`).
#[cfg(any(feature = "host-timers", target_os = "emscripten", target_os = "none"))]
impl Timer {
    fn new() -> Self {
        Self {}
    }

    fn start(&'static self) {}

    fn arm(&self, deadline: Instant, waker: Waker, fired: Arc<AtomicBool>) {
        arm_host_alarm(Alarm {
            deadline,
            waker,
            fired,
        });
    }
}

#[cfg(any(
    feature = "host-timers",
    target_os = "emscripten",
    target_os = "espidf",
    target_os = "none"
))]
fn arm_host_alarm(alarm: Alarm) {
    crate::thread_state::runtimes(|state| state.alarms.borrow_mut().push(alarm));
}

/// When the earliest delay waiting in this thread's runtime is due, for a
/// host that runs its own loop to wake then and drain the UI work. `None`
/// when none waits, and on platforms whose delays wait on a timer of their
/// own.
pub fn next_alarm_deadline() -> Option<Instant> {
    #[cfg(any(
        feature = "host-timers",
        target_os = "emscripten",
        target_os = "espidf",
        target_os = "none"
    ))]
    return crate::thread_state::try_runtimes(|state| {
        state
            .alarms
            .borrow()
            .iter()
            .map(|alarm| alarm.deadline)
            .min()
    })
    .flatten();
    #[cfg(not(any(
        feature = "host-timers",
        target_os = "emscripten",
        target_os = "espidf",
        target_os = "none"
    )))]
    None
}

#[inline]
pub(crate) fn has_due_alarm() -> bool {
    #[cfg(any(
        feature = "host-timers",
        target_os = "emscripten",
        target_os = "espidf",
        target_os = "none"
    ))]
    return next_alarm_deadline().is_some_and(|deadline| deadline <= Instant::now());
    #[cfg(not(any(
        feature = "host-timers",
        target_os = "emscripten",
        target_os = "espidf",
        target_os = "none"
    )))]
    false
}

#[inline]
pub(crate) fn fire_due_alarms() {
    #[cfg(any(
        feature = "host-timers",
        target_os = "emscripten",
        target_os = "espidf",
        target_os = "none"
    ))]
    {
        let now = Instant::now();
        let due: Vec<Alarm> = crate::thread_state::runtimes(|state| {
            let mut alarms = state.alarms.borrow_mut();
            if alarms.iter().all(|alarm| alarm.deadline > now) {
                return Vec::new();
            }
            let (due, waiting) = std::mem::take(&mut *alarms)
                .into_iter()
                .partition(|alarm| alarm.deadline <= now);
            *alarms = waiting;
            due
        });
        for alarm in due {
            alarm.fired.store(true, Ordering::Release);
            alarm.waker.wake();
        }
    }
}
