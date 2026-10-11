//! Background execution: ask the OS to keep running after the app leaves the
//! screen, so work in flight (a download, a save, playback) can finish instead
//! of being suspended at once.
//!
//! Each piece of work holds a [`BackgroundWorkLease`] with a title and, where
//! it knows one, its progress. The platform shows the latest lease where the
//! person can see it: Android in the notification of its foreground service,
//! which opens the app when tapped. iOS shows nothing for this kind of work.
//!
//! The platform backend installs an implementation via
//! [`set_platform_background_activity`] (iOS `beginBackgroundTask`, Android a
//! foreground service). No default: [`background_activity`] returns `None` where
//! unsupported, and the app simply runs only while foregrounded.

use std::sync::{
    Arc, Mutex, MutexGuard, OnceLock, PoisonError,
    atomic::{AtomicUsize, Ordering},
};

/// What the person is shown about work that runs off screen.
#[derive(Clone, Debug, PartialEq)]
pub struct BackgroundWork {
    /// What the work does, as the person reads it: "Downloading the models".
    pub title: String,
    /// How much of it is done, from 0 to 1, when the work knows.
    pub progress: Option<f32>,
}

/// Keeps the app running while work is active, and shows that work.
/// Implementations are `Send + Sync` (leases change from worker threads).
pub trait BackgroundActivity: Send + Sync {
    /// The work to show, or `None` once every lease has ended. Called when the
    /// first lease starts, whenever what is shown changes, and when the last
    /// lease ends; implementations must tolerate repeats.
    fn show(&self, work: Option<&BackgroundWork>);
}

pub type BackgroundActivityRef = Arc<dyn BackgroundActivity>;

fn slot() -> &'static Mutex<Option<BackgroundActivityRef>> {
    static SLOT: OnceLock<Mutex<Option<BackgroundActivityRef>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Installs the platform background-activity handler, replacing any previous one.
pub fn set_platform_background_activity(activity: BackgroundActivityRef) {
    if let Some(work) = leases().shown() {
        activity.show(Some(&work));
    }
    if let Ok(mut s) = slot().lock() {
        *s = Some(activity);
    }
}

/// Removes any registered handler (tests/teardown).
pub fn clear_platform_background_activity() {
    if let Ok(mut s) = slot().lock() {
        *s = None;
    }
}

/// The registered background-activity handler, or `None` where unsupported.
pub fn background_activity() -> Option<BackgroundActivityRef> {
    slot().lock().ok().and_then(|s| s.clone())
}

#[derive(Default)]
struct Leases {
    next: u64,
    /// Active leases, oldest first; the last one is shown.
    active: Vec<(u64, BackgroundWork)>,
}

impl Leases {
    fn shown(&self) -> Option<BackgroundWork> {
        self.active.last().map(|(_, work)| work.clone())
    }
}

static ACTIVE_LEASES: AtomicUsize = AtomicUsize::new(0);

fn leases() -> MutexGuard<'static, Leases> {
    static LEASES: OnceLock<Mutex<Leases>> = OnceLock::new();
    LEASES
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Changes the leases with `change` and tells the platform when what it shows
/// changed. The platform is called outside the lock.
fn update(change: impl FnOnce(&mut Leases)) {
    let (before, after) = {
        let mut leases = leases();
        let before = leases.shown().map(|work| Shown::of(&work));
        change(&mut leases);
        ACTIVE_LEASES.store(leases.active.len(), Ordering::Release);
        (before, leases.shown())
    };
    if before != after.as_ref().map(Shown::of)
        && let Some(activity) = background_activity()
    {
        activity.show(after.as_ref());
    }
}

/// What a platform redraws for: the title and whole percents, so work that
/// reports every chunk does not redraw a notification for each.
#[derive(PartialEq)]
struct Shown {
    title: String,
    percent: Option<u8>,
}

impl Shown {
    fn of(work: &BackgroundWork) -> Self {
        Self {
            title: work.title.clone(),
            percent: work
                .progress
                .map(|fraction| (fraction.clamp(0.0, 1.0) * 100.0) as u8),
        }
    }
}

/// A claim that important work needs the host's background execution allowance.
/// The allowance remains active until every outstanding lease is dropped.
pub struct BackgroundWorkLease {
    id: u64,
}

impl BackgroundWorkLease {
    /// Shows how much of the work is done, from 0 to 1, or `None` when it is
    /// not known.
    pub fn set_progress(&self, progress: Option<f32>) {
        self.change(|work| work.progress = progress);
    }

    /// Changes what the work is called.
    pub fn set_title(&self, title: impl Into<String>) {
        let title = title.into();
        self.change(|work| work.title = title);
    }

    fn change(&self, change: impl FnOnce(&mut BackgroundWork)) {
        update(|leases| {
            if let Some((_, work)) = leases.active.iter_mut().find(|(id, _)| *id == self.id) {
                change(work);
            }
        });
    }
}

impl Drop for BackgroundWorkLease {
    fn drop(&mut self) {
        update(|leases| leases.active.retain(|(id, _)| *id != self.id));
    }
}

/// Acquires background execution for one independent operation, shown to the
/// person as `title` where the platform shows such work.
pub fn acquire_background_work(title: impl Into<String>) -> BackgroundWorkLease {
    let title = title.into();
    let mut id = 0;
    update(|leases| {
        id = leases.next;
        leases.next += 1;
        leases.active.push((
            id,
            BackgroundWork {
                title,
                progress: None,
            },
        ));
    });
    BackgroundWorkLease { id }
}

/// Whether at least one background-work lease is active.
///
/// A platform backend reads this to decide whether the runtime keeps turning
/// while the app is off screen. Both mobile backends drive composition from the
/// frame path, and both stop that path when the surface is gone: Android drops
/// its GPU resources on `TerminateWindow`, iOS stops receiving redraws. Anything
/// posted to the UI dispatcher then waits until the user comes back, which is
/// wrong for an app that told the OS it has work to finish.
pub fn background_active() -> bool {
    ACTIVE_LEASES.load(Ordering::Acquire) != 0
}

#[cfg(test)]
#[path = "tests/background_tests.rs"]
mod tests;
