//! Framework-owned application-host state and controls.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Platform directory roots for application-owned files.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlatformDirectories {
    pub data: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
    pub documents: Option<PathBuf>,
    pub temporary: PathBuf,
    pub shared: Option<PathBuf>,
}

impl PlatformDirectories {
    fn scoped(&self, application_id: &str) -> Self {
        Self {
            data: self.data.join(application_id),
            config: self.config.join(application_id),
            cache: self.cache.join(application_id),
            documents: self
                .documents
                .as_ref()
                .map(|path| path.join(application_id)),
            temporary: self.temporary.join(application_id),
            shared: self.shared.as_ref().map(|path| path.join(application_id)),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PlatformDirectoryError {
    #[error("application id must be one non-empty path component")]
    InvalidApplicationId,
    #[error("no application id has been registered")]
    NoApplicationId,
    #[error("platform directories are unavailable")]
    Unavailable,
}

/// Android-style lifecycle state exposed to composition observers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleState {
    Created,
    Started,
    Resumed,
    Paused,
    Stopped,
    Destroyed,
}

/// A lifecycle transition delivered by the platform host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LifecycleEvent {
    pub from: LifecycleState,
    pub to: LifecycleState,
}

/// Host operations supplied by the framework platform backend.
pub trait HostController: Send + Sync {
    /// Keeps the host window awake while enabled.
    fn set_keep_screen_on(&self, enabled: bool);
    /// Returns platform directory roots inside the application's host sandbox.
    fn platform_directories(&self) -> Option<PlatformDirectories>;
    /// Finishes/backgrounds the host.
    fn exit(&self);
    /// Requests that the app move to the background.
    fn background(&self);
    /// How long this host will wait for durable saves before it suspends the
    /// app. Android's `onPause` and iOS's background transition each allow a
    /// short, platform-defined budget; work that overruns it keeps running
    /// under a background-work lease.
    fn durable_save_deadline(&self) -> std::time::Duration {
        DEFAULT_DURABLE_SAVE_DEADLINE
    }
}

/// The budget used when a host does not state one of its own.
pub const DEFAULT_DURABLE_SAVE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(2);

/// How long the installed host will wait for durable saves.
pub fn durable_save_deadline() -> std::time::Duration {
    host_controller()
        .map(|host| host.durable_save_deadline())
        .unwrap_or(DEFAULT_DURABLE_SAVE_DEADLINE)
}

pub type HostControllerRef = Arc<dyn HostController>;

fn controller() -> &'static Mutex<Option<HostControllerRef>> {
    static SLOT: OnceLock<Mutex<Option<HostControllerRef>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Installs the framework host controller.
pub fn set_host_controller(value: HostControllerRef) {
    if let Ok(mut slot) = controller().lock() {
        *slot = Some(value);
    }
    KEEP_SCREEN_ON.store(0, Ordering::Release);
}
/// Removes the installed host controller.
pub fn clear_host_controller() {
    if let Ok(mut slot) = controller().lock() {
        *slot = None;
    }
    KEEP_SCREEN_ON.store(0, Ordering::Release);
}
/// Returns the installed framework host controller.
pub fn host_controller() -> Option<HostControllerRef> {
    controller().lock().ok().and_then(|slot| slot.clone())
}
/// Enables or disables the host's keep-screen-on flag.
pub fn set_keep_screen_on(enabled: bool) {
    let value = if enabled { 2 } else { 1 };
    if KEEP_SCREEN_ON.swap(value, Ordering::AcqRel) == value {
        return;
    }
    if let Some(host) = host_controller() {
        host.set_keep_screen_on(enabled);
    }
}
fn valid_application_id(application_id: &str) -> bool {
    !application_id.is_empty()
        && Path::new(application_id).components().count() == 1
        && application_id != "."
        && application_id != ".."
}

fn desktop_platform_directories() -> Option<PlatformDirectories> {
    let base = directories::BaseDirs::new()?;
    let documents = directories::UserDirs::new()
        .and_then(|directories| directories.document_dir().map(Path::to_path_buf));
    Some(PlatformDirectories {
        data: base.data_dir().to_path_buf(),
        config: base.config_dir().to_path_buf(),
        cache: base.cache_dir().to_path_buf(),
        documents,
        temporary: std::env::temp_dir(),
        shared: None,
    })
}

fn application_id_slot() -> &'static Mutex<Option<String>> {
    static SLOT: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// Registers the id every framework-owned storage path is scoped by.
///
/// Platform backends call this at startup with what the platform packaged —
/// the Android package name, the iOS bundle identifier, the desktop
/// application name — so applications never assemble a storage path
/// themselves.
pub fn set_application_id(application_id: &str) -> Result<(), PlatformDirectoryError> {
    if !valid_application_id(application_id) {
        return Err(PlatformDirectoryError::InvalidApplicationId);
    }
    if let Ok(mut slot) = application_id_slot().lock() {
        *slot = Some(application_id.to_string());
    }
    Ok(())
}

/// Removes the registered application id (tests and teardown).
pub fn clear_application_id() {
    if let Ok(mut slot) = application_id_slot().lock() {
        *slot = None;
    }
}

/// The registered application id, if a host has published one.
pub fn application_id() -> Option<String> {
    application_id_slot()
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
}

/// Returns typed directories for the registered application.
pub fn application_directories() -> Result<PlatformDirectories, PlatformDirectoryError> {
    let application_id = application_id().ok_or(PlatformDirectoryError::NoApplicationId)?;
    let roots = host_controller()
        .and_then(|host| host.platform_directories())
        .or_else(desktop_platform_directories)
        .ok_or(PlatformDirectoryError::Unavailable)?;
    Ok(roots.scoped(&application_id))
}
/// Requests that the host finish the app.
pub fn exit_app() {
    if let Some(host) = host_controller() {
        host.exit();
    }
}
/// Requests that the host move the app to the background.
pub fn background_app() {
    if let Some(host) = host_controller() {
        host.background();
    }
}

#[cfg(not(target_arch = "wasm32"))]
type Observer = Arc<dyn Fn(LifecycleEvent) + Send + Sync>;
#[cfg(target_arch = "wasm32")]
type Observer = std::rc::Rc<dyn Fn(LifecycleEvent)>;

#[cfg(not(target_arch = "wasm32"))]
fn observers() -> &'static Mutex<Vec<(u64, Observer)>> {
    static SLOT: OnceLock<Mutex<Vec<(u64, Observer)>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(Vec::new()))
}
#[cfg(target_arch = "wasm32")]
thread_local! {
    static OBSERVERS: std::cell::RefCell<Vec<(u64, Observer)>> = const { std::cell::RefCell::new(Vec::new()) };
}
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static LIFECYCLE_STATE: AtomicU8 = AtomicU8::new(LifecycleState::Created as u8);
static KEEP_SCREEN_ON: AtomicU8 = AtomicU8::new(0);

/// RAII lifecycle observer registration.
pub struct LifecycleObserver {
    id: u64,
}
impl Drop for LifecycleObserver {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(mut list) = observers().lock() {
            list.retain(|(id, _)| *id != self.id);
        }
        #[cfg(target_arch = "wasm32")]
        OBSERVERS.with(|list| list.borrow_mut().retain(|(id, _)| *id != self.id));
    }
}
/// Observes host lifecycle transitions until the returned handle is dropped.
#[cfg(not(target_arch = "wasm32"))]
pub fn observe_lifecycle(
    observer: impl Fn(LifecycleEvent) + Send + Sync + 'static,
) -> LifecycleObserver {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut list) = observers().lock() {
        list.push((id, Arc::new(observer)));
    }
    LifecycleObserver { id }
}
/// Observes host lifecycle transitions until the returned handle is dropped.
#[cfg(target_arch = "wasm32")]
pub fn observe_lifecycle(observer: impl Fn(LifecycleEvent) + 'static) -> LifecycleObserver {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    OBSERVERS.with(|list| list.borrow_mut().push((id, std::rc::Rc::new(observer))));
    LifecycleObserver { id }
}
/// Publishes a lifecycle transition from the platform host.
pub fn dispatch_lifecycle(event: LifecycleEvent) {
    LIFECYCLE_STATE.store(event.to as u8, Ordering::Release);
    crate::media::on_lifecycle(event);
    #[cfg(not(target_arch = "wasm32"))]
    let callbacks = observers()
        .lock()
        .map(|list| {
            list.iter()
                .map(|(_, cb)| Arc::clone(cb))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    #[cfg(target_arch = "wasm32")]
    let callbacks = OBSERVERS.with(|list| {
        list.borrow()
            .iter()
            .map(|(_, callback)| std::rc::Rc::clone(callback))
            .collect::<Vec<_>>()
    });
    for callback in callbacks {
        callback(event);
    }
}

/// Returns the latest lifecycle state published by the platform host.
pub fn current_lifecycle_state() -> LifecycleState {
    match LIFECYCLE_STATE.load(Ordering::Acquire) {
        0 => LifecycleState::Created,
        1 => LifecycleState::Started,
        2 => LifecycleState::Resumed,
        3 => LifecycleState::Paused,
        4 => LifecycleState::Stopped,
        _ => LifecycleState::Destroyed,
    }
}

/// Publishes a platform lifecycle state and derives the transition source from
/// the framework's previous state.
///
/// Leaving the foreground runs every registered durable save first, inside the
/// budget the host allows, so an application never has to hook the transition
/// itself to persist its data.
pub fn dispatch_lifecycle_state(to: LifecycleState) {
    let from = current_lifecycle_state();
    if from == to {
        return;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if matches!(to, LifecycleState::Paused) {
        let outcome = run_durable_saves(durable_save_deadline());
        if outcome == DurableSaveOutcome::TimedOut {
            log::warn!("cranpose: durable saves overran the host deadline; they keep running");
        }
    }
    dispatch_lifecycle(LifecycleEvent { from, to });
}

// ---- Observable lifecycle ------------------------------------------------

/// The [`CompositionLocal`] carrying the host's current lifecycle state.
///
/// [`ProvideLifecycle`] installs it; descendants read it and recompose on every
/// transition, so a screen can pause its own work without registering an
/// observer of its own.
pub fn local_lifecycle_state() -> cranpose_core::CompositionLocal<LifecycleState> {
    thread_local! {
        static LOCAL: std::cell::RefCell<Option<cranpose_core::CompositionLocal<LifecycleState>>> =
            const { std::cell::RefCell::new(None) };
    }
    LOCAL.with(|cell| {
        cell.borrow_mut()
            .get_or_insert_with(|| cranpose_core::compositionLocalOf(current_lifecycle_state))
            .clone()
    })
}

/// The host's lifecycle state as observable state.
#[allow(non_snake_case)]
pub fn rememberLifecycleState() -> cranpose_core::State<LifecycleState> {
    let transitions = rememberLifecycleEvents();
    let state = cranpose_core::collectAsState(
        transitions,
        (),
        LifecycleEvent {
            from: current_lifecycle_state(),
            to: current_lifecycle_state(),
        },
    );
    cranpose_core::derivedStateOf(move || state.get().to)
}

/// Host lifecycle transitions as a composition-scoped stream.
#[allow(non_snake_case)]
pub fn rememberLifecycleEvents() -> cranpose_core::EventStream<LifecycleEvent> {
    cranpose_core::rememberEventStream((), |sender| {
        observe_lifecycle(move |event| sender.send(event))
    })
}

/// Provides the host's lifecycle state to descendant composables.
///
/// The application shell wraps its content in this once; screens then read
/// [`local_lifecycle_state`].
#[allow(non_snake_case)]
#[cranpose_macros::composable]
pub fn ProvideLifecycle(content: impl FnOnce()) {
    let state = rememberLifecycleState();
    let local = local_lifecycle_state();
    cranpose_core::CompositionLocalProvider(vec![local.provides(state.get())], move || {
        content();
    });
}

// ---- Durable saves -------------------------------------------------------

/// What became of the durable saves the host asked for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurableSaveOutcome {
    /// Nothing was registered.
    Nothing,
    /// Every registered save finished inside the deadline.
    Completed,
    /// The deadline expired with saves still running. They keep running under
    /// a background-work lease, but the host is free to suspend.
    TimedOut,
}

type SaveWork = Arc<dyn Fn() + Send + Sync>;

fn durable_saves() -> &'static Mutex<Vec<(u64, SaveWork)>> {
    static SLOT: OnceLock<Mutex<Vec<(u64, SaveWork)>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(Vec::new()))
}

/// Keeps a durable save registered until it is dropped.
pub struct DurableSaveRegistration {
    id: u64,
}

impl Drop for DurableSaveRegistration {
    fn drop(&mut self) {
        if let Ok(mut saves) = durable_saves().lock() {
            saves.retain(|(id, _)| *id != self.id);
        }
    }
}

/// Registers work that must reach durable storage before the host suspends.
///
/// Applications use [`DurableSaveEffect`] so the registration is scoped to the
/// composition that owns the data.
pub fn register_durable_save(save: impl Fn() + Send + Sync + 'static) -> DurableSaveRegistration {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut saves) = durable_saves().lock() {
        saves.push((id, Arc::new(save)));
    }
    DurableSaveRegistration { id }
}

/// Registers `save` for as long as this call stays in the composition.
///
/// The host runs it when the app is about to be suspended, off the UI thread
/// and under a background-work lease, so a slow write does not stall the
/// lifecycle callback the platform is waiting on.
#[allow(non_snake_case)]
pub fn DurableSaveEffect<K: PartialEq + 'static>(keys: K, save: impl Fn() + Send + Sync + 'static) {
    cranpose_core::__disposable_effect_impl(
        cranpose_core::location_key(file!(), line!(), column!()),
        keys,
        move |scope| {
            let registration = register_durable_save(save);
            scope.on_dispose(move || drop(registration))
        },
    );
}

/// Runs every registered durable save, waiting up to `deadline`.
///
/// Platform hosts call this from the lifecycle callback the OS gives them —
/// Android's `onPause`, iOS's `applicationDidEnterBackground` — passing the
/// budget that platform allows. Saves run on worker threads under a
/// background-work lease, so work that overruns the deadline still finishes
/// while the OS keeps the process alive.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_durable_saves(deadline: std::time::Duration) -> DurableSaveOutcome {
    let saves: Vec<SaveWork> = durable_saves()
        .lock()
        .map(|saves| saves.iter().map(|(_, save)| Arc::clone(save)).collect())
        .unwrap_or_default();
    if saves.is_empty() {
        return DurableSaveOutcome::Nothing;
    }

    let outstanding = Arc::new(std::sync::atomic::AtomicUsize::new(saves.len()));
    let finished = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    for save in saves {
        let worker_outstanding = Arc::clone(&outstanding);
        let worker_finished = Arc::clone(&finished);
        let lease = crate::background::acquire_background_work();
        let spawned = std::thread::Builder::new()
            .name("cranpose-durable-save".to_string())
            .spawn(move || {
                save();
                drop(lease);
                if worker_outstanding.fetch_sub(1, Ordering::AcqRel) == 1 {
                    let (done, wake) = &*worker_finished;
                    if let Ok(mut done) = done.lock() {
                        *done = true;
                    }
                    wake.notify_all();
                }
            });
        if spawned.is_err() {
            // A host that cannot spawn threads is a host under real pressure;
            // counting the save down keeps the wait from hanging on it.
            log::warn!("cranpose: a durable save could not be started");
            if outstanding.fetch_sub(1, Ordering::AcqRel) == 1 {
                let (done, wake) = &*finished;
                if let Ok(mut done) = done.lock() {
                    *done = true;
                }
                wake.notify_all();
            }
        }
    }

    let (done, wake) = &*finished;
    let Ok(mut guard) = done.lock() else {
        return DurableSaveOutcome::TimedOut;
    };
    let mut remaining = deadline;
    let started = web_time::Instant::now();
    while !*guard {
        let Ok((next, timeout)) = wake.wait_timeout(guard, remaining) else {
            return DurableSaveOutcome::TimedOut;
        };
        guard = next;
        if timeout.timed_out() {
            break;
        }
        remaining = deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            break;
        }
    }
    if *guard {
        DurableSaveOutcome::Completed
    } else {
        DurableSaveOutcome::TimedOut
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|error| error.into_inner())
    }

    #[test]
    fn observer_is_removed_on_drop() {
        let _guard = test_lock();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = Arc::clone(&calls);
        let handle = observe_lifecycle(move |_| {
            seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        dispatch_lifecycle(LifecycleEvent {
            from: LifecycleState::Created,
            to: LifecycleState::Started,
        });
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
        drop(handle);
        dispatch_lifecycle(LifecycleEvent {
            from: LifecycleState::Started,
            to: LifecycleState::Resumed,
        });
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
    }

    #[test]
    fn state_dispatch_derives_the_previous_state() {
        let _guard = test_lock();
        dispatch_lifecycle_state(LifecycleState::Paused);
        assert_eq!(current_lifecycle_state(), LifecycleState::Paused);
        dispatch_lifecycle_state(LifecycleState::Stopped);
        assert_eq!(current_lifecycle_state(), LifecycleState::Stopped);
    }

    #[test]
    fn repeated_keep_screen_value_reaches_the_host_once() {
        let _guard = test_lock();
        struct RecordingHost(Arc<std::sync::atomic::AtomicUsize>);
        impl HostController for RecordingHost {
            fn set_keep_screen_on(&self, _enabled: bool) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
            fn platform_directories(&self) -> Option<PlatformDirectories> {
                Some(PlatformDirectories {
                    data: PathBuf::from("data"),
                    config: PathBuf::from("config"),
                    cache: PathBuf::from("cache"),
                    documents: Some(PathBuf::from("documents")),
                    temporary: PathBuf::from("temporary"),
                    shared: Some(PathBuf::from("shared")),
                })
            }
            fn exit(&self) {}
            fn background(&self) {}
        }
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        set_host_controller(Arc::new(RecordingHost(Arc::clone(&calls))));
        set_keep_screen_on(true);
        set_keep_screen_on(true);
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        set_application_id("sample").expect("a plain id is valid");
        assert_eq!(
            application_directories().unwrap().data,
            PathBuf::from("data/sample")
        );
        clear_application_id();
        clear_host_controller();
    }

    #[test]
    fn durable_saves_run_and_report_completion() {
        // A durable save holds a background-work lease while it runs, and that
        // count is one number for the process, so tests that take or read it
        // take turns.
        let _services = crate::registry::test_service_guard();
        let _guard = test_lock();
        let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let first = Arc::clone(&ran);
        let second = Arc::clone(&ran);
        let a = register_durable_save(move || {
            first.fetch_add(1, Ordering::Relaxed);
        });
        let b = register_durable_save(move || {
            second.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(
            run_durable_saves(std::time::Duration::from_secs(5)),
            DurableSaveOutcome::Completed
        );
        assert_eq!(ran.load(Ordering::Relaxed), 2);
        drop((a, b));
        assert_eq!(
            run_durable_saves(std::time::Duration::from_secs(1)),
            DurableSaveOutcome::Nothing
        );
    }

    #[test]
    fn a_save_that_overruns_the_deadline_reports_a_timeout() {
        let _services = crate::registry::test_service_guard();
        let _guard = test_lock();
        let registration = register_durable_save(|| {
            std::thread::sleep(std::time::Duration::from_millis(400));
        });
        assert_eq!(
            run_durable_saves(std::time::Duration::from_millis(30)),
            DurableSaveOutcome::TimedOut
        );
        drop(registration);
    }

    #[test]
    fn a_dropped_registration_is_no_longer_saved() {
        let _services = crate::registry::test_service_guard();
        let _guard = test_lock();
        let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = Arc::clone(&ran);
        let registration = register_durable_save(move || {
            counted.fetch_add(1, Ordering::Relaxed);
        });
        drop(registration);
        assert_eq!(
            run_durable_saves(std::time::Duration::from_secs(1)),
            DurableSaveOutcome::Nothing
        );
        assert_eq!(ran.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn application_id_must_be_one_component() {
        let _guard = test_lock();
        assert_eq!(
            set_application_id("../sample"),
            Err(PlatformDirectoryError::InvalidApplicationId)
        );
        assert_eq!(
            set_application_id(""),
            Err(PlatformDirectoryError::InvalidApplicationId)
        );
        clear_application_id();
        assert_eq!(
            application_directories(),
            Err(PlatformDirectoryError::NoApplicationId)
        );
    }
}
