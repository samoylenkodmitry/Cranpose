use std::sync::PoisonError;

use super::*;

fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(PoisonError::into_inner)
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

#[test]
fn a_surviving_durable_save_keeps_its_registration_when_a_leader_leaves() {
    let _guard = test_lock();
    durable_saves()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
    let ran: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
    let show_first = std::rc::Rc::new(std::cell::Cell::new(true));

    fn saves(show_first: bool, ran: &Arc<Mutex<Vec<&'static str>>>) {
        if show_first {
            let ran = Arc::clone(ran);
            DurableSaveEffect((), move || {
                ran.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push("first");
            });
        }
        let ran = Arc::clone(ran);
        DurableSaveEffect((), move || {
            ran.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push("tail");
        });
    }

    let mut composition = cranpose_core::Composition::new(cranpose_core::MemoryApplier::new());
    let root_key = cranpose_core::location_key(file!(), line!(), column!());
    let mut pass = {
        let ran = Arc::clone(&ran);
        let show_first = std::rc::Rc::clone(&show_first);
        move || saves(show_first.get(), &ran)
    };

    composition
        .render(root_key, &mut pass)
        .expect("initial composition");
    show_first.set(false);
    composition
        .render(root_key, &mut pass)
        .expect("drop the leading save");

    let outcome = run_durable_saves(std::time::Duration::from_secs(5));
    assert_eq!(outcome, DurableSaveOutcome::Completed);
    assert_eq!(
        ran.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_slice(),
        ["tail"],
        "the surviving effect must keep its own registration; adopting the \
         departed leader's group keeps the wrong save alive"
    );
}
