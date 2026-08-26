//! Composition-owned launchers for the system choosers.
//!
//! A launcher is remembered under a stable **request key**, presents a system
//! chooser when the application calls `launch`, and delivers the result to the
//! composition through a callback. The framework — not the application — owns
//! the awkward part: Android may destroy the activity, and with it the whole
//! composition, while the chooser is in front. The launcher records which
//! request is in flight, the platform backend records the granted selection,
//! and the launcher that recomposes under the same key receives the result.
//! Applications never poll an inbox, never drain a resume queue, and never
//! write a marker file.
//!
//! ```rust,no_run
//! use cranpose_macros::composable;
//! use cranpose_services::{launcher::rememberOpenFileLauncher, FilePickerOptions};
//!
//! #[composable]
//! fn ImportButton() {
//!     let launcher = rememberOpenFileLauncher("import.document", |picked| {
//!         if let Ok(Some(content)) = picked {
//!             log::info!("picked {}", content.metadata().name);
//!         }
//!     });
//!     launcher.launch(FilePickerOptions::default().with_title("Import"));
//! }
//! ```

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    rc::Rc,
};

use cranpose_core::{RuntimeHandle, current_runtime_handle};
use cranpose_macros::composable;

use crate::{
    content::{ContentFolderRef, ContentHandle, ContentSinkRef},
    file_picker::{
        FilePickerError, FilePickerOptions, FilePickerRef, RecoveredPick, SaveDocumentRequest,
        local_file_picker,
    },
    preferences::preferences,
};

/// What a launcher hands back once the chooser resolves.
///
/// Cancellation is `Ok` with an empty selection, not an error: the user
/// declining is an outcome, not a failure.
pub type LauncherResult<T> = Result<T, FilePickerError>;

thread_local! {
    /// Results the host recovered after their requesting composition was
    /// destroyed, keyed by the request key that asked for them.
    static RECOVERED: RefCell<HashMap<String, RecoveredPick>> = RefCell::new(HashMap::new());
    /// The request key of the chooser currently in flight, if any. At most one
    /// system chooser can be in front, so one slot is enough.
    static IN_FLIGHT: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Request keys with a live launcher, so duplicates are caught in debug.
    static REGISTERED_KEYS: RefCell<HashMap<String, usize>> = RefCell::new(HashMap::new());
}

/// Where the in-flight request key is kept so it outlives the process.
///
/// The in-memory slot covers a composition being torn down. It does not cover
/// the process being killed, which is the case Android actually presents: the
/// system may destroy an application outright while its chooser is in front,
/// and the grant then arrives to a process that has forgotten it asked. An
/// application that wanted to survive that had to write its own marker file
/// beside its data and read it back on the next start — which is the framework
/// doing half a job and leaving the awkward half to every application.
const IN_FLIGHT_PREFERENCE: &str = "cranpose.launcher.in-flight";

/// Records that `request_key` launched a chooser. Called before presenting so a
/// host recreation — or a restart after the process was killed — can attribute
/// the recovered result.
fn begin_request(request_key: &str) {
    IN_FLIGHT.with(|slot| *slot.borrow_mut() = Some(request_key.to_string()));
    // Written before the chooser is presented, so a process killed while it is
    // in front still leaves the record behind.
    let _ = preferences().set(IN_FLIGHT_PREFERENCE, request_key);
}

/// Clears the in-flight record once the chooser resolves in this process.
fn finish_request(request_key: &str) {
    IN_FLIGHT.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.as_deref() == Some(request_key) {
            *slot = None;
        }
    });
    if preferences().get(IN_FLIGHT_PREFERENCE).as_deref() == Some(request_key) {
        let _ = preferences().remove(IN_FLIGHT_PREFERENCE);
    }
}

/// The request that was in flight, from this process or the one before it.
fn in_flight_request() -> Option<String> {
    IN_FLIGHT
        .with(|slot| slot.borrow().clone())
        .or_else(|| preferences().get(IN_FLIGHT_PREFERENCE))
}

/// Moves anything the platform recovered into the keyed inbox. Called whenever a
/// launcher composes, which is the first moment a recreated composition can take
/// delivery.
fn drain_recovered(picker: &FilePickerRef) {
    while let Some(pick) = picker.take_recovered_pick() {
        let Some(key) = in_flight_request() else {
            log::warn!("cranpose: a recovered pick arrived with no request in flight; dropping it");
            continue;
        };
        RECOVERED.with(|inbox| inbox.borrow_mut().insert(key, pick));
    }
}

/// Takes the recovered result addressed to `request_key`, if any.
fn take_recovered(request_key: &str) -> Option<RecoveredPick> {
    let recovered = RECOVERED.with(|inbox| inbox.borrow_mut().remove(request_key));
    if recovered.is_some() {
        finish_request(request_key);
    }
    recovered
}

/// Discards every framework-owned launcher record. Used by tests and by host
/// teardown so one composition's in-flight request never leaks into the next.
pub fn clear_launcher_state() {
    RECOVERED.with(|inbox| inbox.borrow_mut().clear());
    IN_FLIGHT.with(|slot| *slot.borrow_mut() = None);
    REGISTERED_KEYS.with(|keys| keys.borrow_mut().clear());
}

/// Shared state behind every launcher: the picker, the runtime that runs the
/// chooser future, and whether a request is outstanding.
struct LauncherCore {
    request_key: String,
    picker: FilePickerRef,
    runtime: Option<RuntimeHandle>,
    in_flight: Cell<bool>,
}

impl LauncherCore {
    fn spawn(self: &Rc<Self>, future: impl Future<Output = ()> + 'static) {
        let Some(runtime) = self.runtime.clone() else {
            log::warn!(
                "cranpose: launcher `{}` has no runtime; the chooser was not presented",
                self.request_key
            );
            self.in_flight.set(false);
            finish_request(&self.request_key);
            return;
        };
        if runtime.spawn_ui(future).is_none() {
            log::warn!(
                "cranpose: launcher `{}` outlived its runtime; the chooser was not presented",
                self.request_key
            );
            self.in_flight.set(false);
            finish_request(&self.request_key);
        }
    }

    /// Marks a request as starting, refusing to present a second chooser while
    /// one is already in front.
    fn begin(self: &Rc<Self>) -> bool {
        if self.in_flight.get() {
            return false;
        }
        self.in_flight.set(true);
        begin_request(&self.request_key);
        true
    }

    fn end(self: &Rc<Self>) {
        self.in_flight.set(false);
        finish_request(&self.request_key);
    }
}

/// Registration guard: keeps the duplicate-key check honest across recomposition
/// and removes the key when the launcher leaves the composition.
struct KeyRegistration {
    request_key: String,
}

impl KeyRegistration {
    fn new(request_key: &str) -> Self {
        REGISTERED_KEYS.with(|keys| {
            let mut keys = keys.borrow_mut();
            let count = keys.entry(request_key.to_string()).or_insert(0);
            *count += 1;
            debug_assert!(
                *count == 1,
                "launcher request key `{request_key}` is registered {count} times; \
                 keys must be unique so a recovered result reaches the right launcher"
            );
        });
        Self {
            request_key: request_key.to_string(),
        }
    }
}

impl Drop for KeyRegistration {
    fn drop(&mut self) {
        REGISTERED_KEYS.with(|keys| {
            let mut keys = keys.borrow_mut();
            if let Some(count) = keys.get_mut(&self.request_key) {
                *count -= 1;
                if *count == 0 {
                    keys.remove(&self.request_key);
                }
            }
        });
    }
}

/// Everything a remembered launcher owns for one request key.
struct LauncherSlot {
    core: Rc<LauncherCore>,
    _registration: KeyRegistration,
}

/// Remembers a launcher core for `request_key`, draining anything the host
/// recovered for it and handing it to `deliver`.
fn remember_core(
    request_key: &'static str,
    deliver: impl FnOnce(RecoveredPick) + 'static,
) -> Rc<LauncherCore> {
    let picker = local_file_picker().current();
    let slot = cranpose_core::remember({
        let picker = picker.clone();
        let request_key = request_key.to_string();
        move || LauncherSlot {
            core: Rc::new(LauncherCore {
                request_key: request_key.clone(),
                picker,
                runtime: current_runtime_handle(),
                in_flight: Cell::new(false),
            }),
            _registration: KeyRegistration::new(&request_key),
        }
    });
    let core = slot.with(|slot| Rc::clone(&slot.core));

    drain_recovered(&picker);
    if let Some(recovered) = take_recovered(request_key) {
        let core = Rc::clone(&core);
        cranpose_core::SideEffect(move || {
            core.end();
            deliver(recovered);
        });
    }
    core
}

/// Presents the single-file chooser and hands the picked content back.
#[derive(Clone)]
pub struct OpenFileLauncher {
    core: Rc<LauncherCore>,
    on_result: Rc<dyn Fn(LauncherResult<Option<ContentHandle>>)>,
}

impl OpenFileLauncher {
    /// Presents the chooser. A second call while one is in front is ignored.
    pub fn launch(&self, options: FilePickerOptions) {
        if !self.core.begin() {
            return;
        }
        let core = Rc::clone(&self.core);
        let done = Rc::clone(&self.core);
        let on_result = Rc::clone(&self.on_result);
        let future = core.picker.pick_file(options);
        core.spawn(async move {
            let result = future.await;
            done.end();
            on_result(result);
        });
    }

    /// Whether a chooser presented by this launcher is still in front.
    pub fn is_in_flight(&self) -> bool {
        self.core.in_flight.get()
    }
}

/// Remembers a single-file launcher under `request_key`.
#[allow(non_snake_case)]
#[composable]
pub fn rememberOpenFileLauncher(
    request_key: &'static str,
    on_result: impl Fn(LauncherResult<Option<ContentHandle>>) + 'static,
) -> OpenFileLauncher {
    let on_result: Rc<dyn Fn(LauncherResult<Option<ContentHandle>>)> = Rc::new(on_result);
    let core = remember_core(request_key, {
        let on_result = Rc::clone(&on_result);
        move |recovered| match recovered {
            RecoveredPick::File(content) => on_result(Ok(Some(content))),
            RecoveredPick::Files(mut files) => on_result(Ok(files.drain(..).next())),
            _ => log::warn!("cranpose: `{request_key}` recovered a pick of another kind"),
        }
    });
    OpenFileLauncher { core, on_result }
}

/// Presents the multi-file chooser and hands every picked item back.
#[derive(Clone)]
pub struct OpenFilesLauncher {
    core: Rc<LauncherCore>,
    on_result: Rc<dyn Fn(LauncherResult<Vec<ContentHandle>>)>,
}

impl OpenFilesLauncher {
    /// Presents the chooser. A second call while one is in front is ignored.
    pub fn launch(&self, options: FilePickerOptions) {
        if !self.core.begin() {
            return;
        }
        let core = Rc::clone(&self.core);
        let done = Rc::clone(&self.core);
        let on_result = Rc::clone(&self.on_result);
        let future = core.picker.pick_files(options);
        core.spawn(async move {
            let result = future.await;
            done.end();
            on_result(result);
        });
    }

    /// Whether a chooser presented by this launcher is still in front.
    pub fn is_in_flight(&self) -> bool {
        self.core.in_flight.get()
    }
}

/// Remembers a multi-file launcher under `request_key`.
#[allow(non_snake_case)]
#[composable]
pub fn rememberOpenFilesLauncher(
    request_key: &'static str,
    on_result: impl Fn(LauncherResult<Vec<ContentHandle>>) + 'static,
) -> OpenFilesLauncher {
    let on_result: Rc<dyn Fn(LauncherResult<Vec<ContentHandle>>)> = Rc::new(on_result);
    let core = remember_core(request_key, {
        let on_result = Rc::clone(&on_result);
        move |recovered| match recovered {
            RecoveredPick::Files(files) => on_result(Ok(files)),
            RecoveredPick::File(content) => on_result(Ok(vec![content])),
            _ => log::warn!("cranpose: `{request_key}` recovered a pick of another kind"),
        }
    });
    OpenFilesLauncher { core, on_result }
}

/// Presents the folder chooser and hands the granted folder back.
#[derive(Clone)]
pub struct OpenFolderLauncher {
    core: Rc<LauncherCore>,
    on_result: Rc<dyn Fn(LauncherResult<Option<ContentFolderRef>>)>,
}

impl OpenFolderLauncher {
    /// Presents the chooser. A second call while one is in front is ignored.
    pub fn launch(&self, options: FilePickerOptions) {
        if !self.core.begin() {
            return;
        }
        let core = Rc::clone(&self.core);
        let done = Rc::clone(&self.core);
        let on_result = Rc::clone(&self.on_result);
        let future = core.picker.pick_folder(options);
        core.spawn(async move {
            let result = future.await;
            done.end();
            on_result(result);
        });
    }

    /// Whether a chooser presented by this launcher is still in front.
    pub fn is_in_flight(&self) -> bool {
        self.core.in_flight.get()
    }
}

/// Remembers a folder launcher under `request_key`.
#[allow(non_snake_case)]
#[composable]
pub fn rememberOpenFolderLauncher(
    request_key: &'static str,
    on_result: impl Fn(LauncherResult<Option<ContentFolderRef>>) + 'static,
) -> OpenFolderLauncher {
    let on_result: Rc<dyn Fn(LauncherResult<Option<ContentFolderRef>>)> = Rc::new(on_result);
    let core = remember_core(request_key, {
        let on_result = Rc::clone(&on_result);
        move |recovered| match recovered {
            RecoveredPick::Folder(folder) => on_result(Ok(Some(folder))),
            _ => log::warn!("cranpose: `{request_key}` recovered a pick of another kind"),
        }
    });
    OpenFolderLauncher { core, on_result }
}

/// Presents the save-document chooser and hands the opened sink back.
#[derive(Clone)]
pub struct SaveDocumentLauncher {
    core: Rc<LauncherCore>,
    on_result: Rc<dyn Fn(LauncherResult<Option<ContentSinkRef>>)>,
}

impl SaveDocumentLauncher {
    /// Presents the chooser. A second call while one is in front is ignored.
    pub fn launch(&self, request: SaveDocumentRequest) {
        if !self.core.begin() {
            return;
        }
        let core = Rc::clone(&self.core);
        let done = Rc::clone(&self.core);
        let on_result = Rc::clone(&self.on_result);
        let future = core.picker.save_document(request);
        core.spawn(async move {
            let result = future.await;
            done.end();
            on_result(result);
        });
    }

    /// Whether a chooser presented by this launcher is still in front.
    pub fn is_in_flight(&self) -> bool {
        self.core.in_flight.get()
    }
}

/// Remembers a save-document launcher under `request_key`.
#[allow(non_snake_case)]
#[composable]
pub fn rememberSaveDocumentLauncher(
    request_key: &'static str,
    on_result: impl Fn(LauncherResult<Option<ContentSinkRef>>) + 'static,
) -> SaveDocumentLauncher {
    let on_result: Rc<dyn Fn(LauncherResult<Option<ContentSinkRef>>)> = Rc::new(on_result);
    let core = remember_core(request_key, move |_recovered| {
        log::warn!("cranpose: `{request_key}` recovered a pick of another kind");
    });
    SaveDocumentLauncher { core, on_result }
}

/// Presents the persistent writable-folder chooser and hands the durable handle
/// back. Store the handle and reopen it with
/// [`crate::writable_folder::open_writable_folder`].
#[derive(Clone)]
pub struct WritableFolderLauncher {
    core: Rc<LauncherCore>,
    on_result: Rc<dyn Fn(LauncherResult<Option<String>>)>,
}

impl WritableFolderLauncher {
    /// Presents the chooser. A second call while one is in front is ignored.
    pub fn launch(&self, options: FilePickerOptions) {
        if !self.core.begin() {
            return;
        }
        let core = Rc::clone(&self.core);
        let done = Rc::clone(&self.core);
        let on_result = Rc::clone(&self.on_result);
        let future = core.picker.pick_writable_folder(options);
        core.spawn(async move {
            let result = future.await;
            done.end();
            on_result(result);
        });
    }

    /// Whether a chooser presented by this launcher is still in front.
    pub fn is_in_flight(&self) -> bool {
        self.core.in_flight.get()
    }
}

/// Remembers a writable-folder launcher under `request_key`.
#[allow(non_snake_case)]
#[composable]
pub fn rememberWritableFolderLauncher(
    request_key: &'static str,
    on_result: impl Fn(LauncherResult<Option<String>>) + 'static,
) -> WritableFolderLauncher {
    let on_result: Rc<dyn Fn(LauncherResult<Option<String>>)> = Rc::new(on_result);
    let core = remember_core(request_key, {
        let on_result = Rc::clone(&on_result);
        move |recovered| match recovered {
            RecoveredPick::WritableFolder(handle) => on_result(Ok(Some(handle))),
            _ => log::warn!("cranpose: `{request_key}` recovered a pick of another kind"),
        }
    });
    WritableFolderLauncher { core, on_result }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The persistence contract, in one test.
    ///
    /// Two tests would share the process-wide preferences store *and* the one
    /// key the in-flight record lives under, and the harness runs them at the
    /// same time - so each would clear the other's record and the failure would
    /// look like the contract being wrong. One outstanding request is also the
    /// real invariant: a system chooser is modal, so there is one to remember.
    #[test]
    fn an_in_flight_request_outlives_the_process_that_started_it() {
        // An in-memory store, because a unit test has no business writing to
        // the user's config directory - which is where the default file-backed
        // store puts it.
        crate::preferences::set_platform_preferences(std::sync::Arc::new(
            crate::preferences::MemoryPreferences::new(),
        ));

        // The in-memory slot is what a torn-down composition leaves behind. A
        // killed process leaves nothing, so the record has to be somewhere that
        // outlives it - otherwise a grant returns to an application that has
        // forgotten it asked, and every application writes its own marker file.
        begin_request("test.pick");
        assert_eq!(in_flight_request().as_deref(), Some("test.pick"));

        // The process dies: the thread-local is gone, the record is not.
        IN_FLIGHT.with(|slot| *slot.borrow_mut() = None);
        assert_eq!(
            in_flight_request().as_deref(),
            Some("test.pick"),
            "a restarted process must still know which request was outstanding"
        );

        // A different launcher resolving must not clear this one's record;
        // `drain_recovered` routes a recovered grant by it.
        finish_request("test.other");
        assert_eq!(
            in_flight_request().as_deref(),
            Some("test.pick"),
            "one launcher resolving must not clear a different launcher's record"
        );

        finish_request("test.pick");
        assert_eq!(
            in_flight_request(),
            None,
            "a resolved request is not still in flight"
        );

        crate::preferences::clear_platform_preferences();
    }
}
