//! The launcher contract: a chooser result reaches the composition that asked
//! for it, including when the host destroyed and rebuilt that composition while
//! the chooser was in front.

use cranpose_core::{location_key, Composition, MemoryApplier};
use cranpose_services::{
    clear_launcher_state, clear_platform_file_picker, set_platform_file_picker, BytesContent,
    ContentFolderRef, ContentHandle, FilePicker, FilePickerError, FilePickerOptions,
    LauncherResult, PickerFuture, RecoveredPick,
};
use std::cell::RefCell;
use std::rc::Rc;

/// A picker whose chooser never resolves in this process, and which instead
/// reports the selection the host recovered — exactly what Android does when it
/// recreates the activity while the Storage Access Framework is in front.
#[derive(Default)]
struct RecreatingPicker {
    recovered: RefCell<Vec<RecoveredPick>>,
    launches: std::cell::Cell<usize>,
}

impl FilePicker for RecreatingPicker {
    fn pick_file(
        &self,
        _options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<ContentHandle>, FilePickerError>> {
        self.launches.set(self.launches.get() + 1);
        // The composition is destroyed before this ever resolves.
        Box::pin(std::future::pending())
    }

    fn pick_folder(
        &self,
        _options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<ContentFolderRef>, FilePickerError>> {
        Box::pin(async { Ok(None) })
    }

    fn take_recovered_pick(&self) -> Option<RecoveredPick> {
        self.recovered.borrow_mut().pop()
    }
}

/// An immediately-resolving picker, for the ordinary path.
struct InstantPicker {
    name: &'static str,
}

impl FilePicker for InstantPicker {
    fn pick_file(
        &self,
        _options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<ContentHandle>, FilePickerError>> {
        let name = self.name;
        Box::pin(async move { Ok(Some(BytesContent::named(name, b"picked".to_vec()).handle())) })
    }

    fn pick_folder(
        &self,
        _options: FilePickerOptions,
    ) -> PickerFuture<Result<Option<ContentFolderRef>, FilePickerError>> {
        Box::pin(async { Ok(None) })
    }
}

fn render(build: impl FnMut()) -> Composition<MemoryApplier> {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), build)
        .expect("the composition renders");
    composition
}

#[test]
fn a_recovered_pick_reaches_the_launcher_that_asked_for_it() {
    clear_platform_file_picker();
    clear_launcher_state();

    let picker = Rc::new(RecreatingPicker::default());
    set_platform_file_picker(picker.clone());

    // First composition: the button is pressed and the chooser is presented.
    {
        let launched = Rc::new(std::cell::Cell::new(false));
        let launched_in_build = Rc::clone(&launched);
        let composition = render(move || {
            let launcher = cranpose_services::rememberOpenFileLauncher(
                "test.open-file",
                |_: LauncherResult<Option<ContentHandle>>| {
                    panic!("the first composition never receives a result");
                },
            );
            if !launched_in_build.replace(true) {
                launcher.launch(FilePickerOptions::default());
            }
        });
        drop(composition);
    }
    assert_eq!(picker.launches.get(), 1);

    // The host recreated the activity; the platform recorded the grant.
    picker.recovered.borrow_mut().push(RecoveredPick::File(
        BytesContent::named("recovered.txt", b"recovered".to_vec()).handle(),
    ));

    // Second composition: the same request key takes delivery, with no polling
    // and no application-side inbox.
    let delivered: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let seen = Rc::clone(&delivered);
    let _composition = render(move || {
        let seen = Rc::clone(&seen);
        cranpose_services::rememberOpenFileLauncher("test.open-file", move |picked| {
            if let Ok(Some(content)) = picked {
                *seen.borrow_mut() = Some(content.metadata().name);
            }
        });
    });

    assert_eq!(delivered.borrow().as_deref(), Some("recovered.txt"));
    clear_platform_file_picker();
    clear_launcher_state();
}

#[test]
fn a_recovered_pick_is_delivered_once() {
    clear_platform_file_picker();
    clear_launcher_state();

    let picker = Rc::new(RecreatingPicker::default());
    set_platform_file_picker(picker.clone());

    // Mark a request in flight, then hand the framework its recovered result.
    {
        let launched = Rc::new(std::cell::Cell::new(false));
        let launched_in_build = Rc::clone(&launched);
        drop(render(move || {
            let launcher = cranpose_services::rememberOpenFileLauncher(
                "test.once",
                |_: LauncherResult<Option<ContentHandle>>| {},
            );
            if !launched_in_build.replace(true) {
                launcher.launch(FilePickerOptions::default());
            }
        }));
    }
    picker.recovered.borrow_mut().push(RecoveredPick::File(
        BytesContent::named("once.txt", b"once".to_vec()).handle(),
    ));

    let deliveries = Rc::new(std::cell::Cell::new(0usize));
    let counted = Rc::clone(&deliveries);
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let build = move || {
        let counted = Rc::clone(&counted);
        cranpose_services::rememberOpenFileLauncher("test.once", move |picked| {
            if matches!(picked, Ok(Some(_))) {
                counted.set(counted.get() + 1);
            }
        });
    };
    composition
        .render(key, build.clone())
        .expect("first render");
    composition
        .render(key, build)
        .expect("a recomposition does not replay the result");
    assert_eq!(deliveries.get(), 1);

    clear_platform_file_picker();
    clear_launcher_state();
}

#[test]
fn an_ordinary_pick_resolves_through_the_callback() {
    clear_platform_file_picker();
    clear_launcher_state();
    set_platform_file_picker(Rc::new(InstantPicker { name: "live.txt" }));

    let delivered: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let seen = Rc::clone(&delivered);
    let launched = Rc::new(std::cell::Cell::new(false));
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), move || {
            let seen = Rc::clone(&seen);
            let launcher =
                cranpose_services::rememberOpenFileLauncher("test.live", move |picked| {
                    if let Ok(Some(content)) = picked {
                        *seen.borrow_mut() = Some(content.metadata().name);
                    }
                });
            if !launched.replace(true) {
                launcher.launch(FilePickerOptions::default());
            }
        })
        .expect("the composition renders");
    composition.runtime_handle().drain_ui();

    assert_eq!(delivered.borrow().as_deref(), Some("live.txt"));
    clear_platform_file_picker();
    clear_launcher_state();
}

/// A chooser is a modal the user is standing in front of. `is_in_flight` is
/// what a screen reads to grey its own button out; if it stayed true after the
/// result arrived, the button would never come back, and if a second launch
/// went through while one was up the application would present two choosers.
#[test]
fn a_launcher_reports_a_chooser_that_is_still_in_front() {
    clear_platform_file_picker();
    clear_launcher_state();
    let picker = Rc::new(RecreatingPicker::default());
    set_platform_file_picker(picker.clone());

    let state: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let launched = Rc::new(std::cell::Cell::new(false));
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let seen = Rc::clone(&state);
    let mut render = move || {
        let launcher = cranpose_services::rememberOpenFileLauncher("test.inflight", |_| {});
        seen.borrow_mut().push(launcher.is_in_flight());
        if !launched.replace(true) {
            launcher.launch(FilePickerOptions::default());
            // A second launch while the first chooser is still up must not
            // present another one.
            launcher.launch(FilePickerOptions::default());
            seen.borrow_mut().push(launcher.is_in_flight());
        }
    };
    composition.render(key, &mut render).expect("first render");
    composition.runtime_handle().drain_ui();

    assert_eq!(
        *state.borrow(),
        vec![false, true],
        "a launcher is idle until it presents a chooser, and busy while one is up"
    );
    assert_eq!(
        picker.launches.get(),
        1,
        "a launcher with a chooser already in front must not present a second"
    );

    clear_platform_file_picker();
    clear_launcher_state();
}

/// The ordinary path: once the chooser resolves, the launcher is idle again.
#[test]
fn a_launcher_is_idle_again_once_the_chooser_resolves() {
    clear_platform_file_picker();
    clear_launcher_state();
    set_platform_file_picker(Rc::new(InstantPicker { name: "done.txt" }));

    let after: Rc<std::cell::Cell<bool>> = Rc::new(std::cell::Cell::new(true));
    let launched = Rc::new(std::cell::Cell::new(false));
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());
    let seen = Rc::clone(&after);
    let mut render = move || {
        let launcher = cranpose_services::rememberOpenFileLauncher("test.settled", |_| {});
        if !launched.replace(true) {
            launcher.launch(FilePickerOptions::default());
        }
        seen.set(launcher.is_in_flight());
    };
    composition.render(key, &mut render).expect("first render");
    composition.runtime_handle().drain_ui();
    composition.render(key, &mut render).expect("second render");

    assert!(
        !after.get(),
        "a launcher whose chooser has resolved must report itself idle"
    );
    clear_platform_file_picker();
    clear_launcher_state();
}
