use std::{cell::RefCell, rc::Rc};

use cranpose_core::{Composition, MemoryApplier, location_key};
use cranpose_services::{
    BytesContent, ContentFolderRef, ContentHandle, FilePicker, FilePickerError, FilePickerOptions,
    LauncherResult, PickerFuture, RecoveredPick, clear_launcher_state, clear_platform_file_picker,
    set_platform_file_picker,
};

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

    picker.recovered.borrow_mut().push(RecoveredPick::File(
        BytesContent::named("recovered.txt", b"recovered".to_vec()).handle(),
    ));

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
