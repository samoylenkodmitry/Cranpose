use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

mod support;

use coroflow::{FlowExt, MainScope, MutableSharedFlow, MutableStateFlow, StateFlow};
use cranpose_core::{Composition, MemoryApplier, mutableStateOf, remember};
use cranpose_coroflow::{
    CollectFlow, Handle, StateFlowCollect, main_dispatcher, rememberViewModel, snapshotFlow,
};
use support::{PATIENCE, QUIET_PERIOD, composition, pump_for, pump_until};

struct DropMarker(Arc<AtomicUsize>);

impl Drop for DropMarker {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn the_main_dispatcher_runs_coroutines_on_the_ui_queue() {
    let mut composition = composition();
    let scope: Rc<RefCell<Option<MainScope>>> = Rc::default();
    let capture = Rc::clone(&scope);
    composition
        .render(1, move || {
            let dispatcher = main_dispatcher();
            assert!(dispatcher.is_some());
            if let Some(dispatcher) = dispatcher {
                *capture.borrow_mut() = Some(MainScope::new(dispatcher));
            }
        })
        .expect("render");
    let ran = Rc::new(RefCell::new(false));
    let flag = Rc::clone(&ran);
    if let Some(scope) = scope.borrow().as_ref() {
        scope.launch(async move {
            *flag.borrow_mut() = true;
        });
    }
    assert!(!*ran.borrow(), "launch only queues the coroutine");
    composition.runtime_handle().drain_ui();
    assert!(*ran.borrow());
}

#[test]
fn collect_as_state_follows_a_state_flow_set_from_another_thread() {
    let mut composition = composition();
    let source = MutableStateFlow::new(1);
    let flow = source.as_state_flow();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let render = {
        let seen = Rc::clone(&seen);
        move |composition: &mut Composition<MemoryApplier>| {
            let seen = Rc::clone(&seen);
            let flow = flow.clone();
            composition
                .render(1, move || {
                    seen.borrow_mut().push(flow.collectAsState().get());
                })
                .expect("render");
        }
    };
    let render_again = render.clone();
    render_again(&mut composition);
    assert_eq!(
        seen.borrow().first(),
        Some(&1),
        "the first frame reads the current value"
    );

    let writer = source.clone();
    std::thread::spawn(move || writer.set(7))
        .join()
        .expect("writer thread");
    assert!(pump_until(&mut composition, render, || seen
        .borrow()
        .last()
        == Some(&7)));
    assert_eq!(source.subscription_count(), 1);

    composition.render(1, || {}).expect("remove");
    composition.runtime_handle().drain_ui();
    assert_eq!(
        source.subscription_count(),
        0,
        "leaving the composition unsubscribes"
    );
}

struct CounterViewModel {
    scope: MainScope,
    count: MutableStateFlow<u32>,
}

impl CounterViewModel {
    fn new(scope: MainScope, dropped: Arc<AtomicUsize>) -> Self {
        scope.launch(async move {
            let _marker = DropMarker(dropped);
            std::future::pending::<()>().await;
        });
        Self {
            scope,
            count: MutableStateFlow::new(0),
        }
    }

    fn count(&self) -> StateFlow<u32> {
        self.count.as_state_flow()
    }

    fn increment(&self) {
        let count = self.count.clone();
        self.scope.launch(async move {
            count.update(|value| value + 1);
        });
    }
}

#[test]
fn a_remembered_view_model_survives_recomposition_and_is_cleared_on_removal() {
    let mut composition = composition();
    let dropped = Arc::new(AtomicUsize::new(0));
    let models: Rc<RefCell<Vec<Handle<CounterViewModel>>>> = Rc::default();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let render = {
        let (dropped, models, seen) = (Arc::clone(&dropped), Rc::clone(&models), Rc::clone(&seen));
        move |composition: &mut Composition<MemoryApplier>| {
            let (dropped, models, seen) =
                (Arc::clone(&dropped), Rc::clone(&models), Rc::clone(&seen));
            composition
                .render(1, move || {
                    let dropped = Arc::clone(&dropped);
                    let model =
                        rememberViewModel(move |scope| CounterViewModel::new(scope, dropped));
                    seen.borrow_mut()
                        .push(model.get().count().collectAsState().get());
                    models.borrow_mut().push(model);
                })
                .expect("render");
        }
    };
    let first = render.clone();
    first(&mut composition);
    let second = render.clone();
    second(&mut composition);
    {
        let models = models.borrow();
        assert!(models[0] == models[1], "one view model per position");
        assert!(Rc::ptr_eq(&models[0].get(), &models[1].get()));
        models[0].get().increment();
    }
    assert!(pump_until(&mut composition, render, || seen
        .borrow()
        .last()
        == Some(&1)));
    models.borrow_mut().clear();

    composition.render(1, || {}).expect("remove");
    composition.runtime_handle().drain_ui();
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        1,
        "clearing the view model cancels its scope"
    );
}

#[test]
fn collect_flow_delivers_events_and_restarts_when_its_key_changes() {
    let mut composition = composition();
    let events = MutableSharedFlow::new(0, 8);
    let received = Rc::new(RefCell::new(Vec::new()));
    let key = Rc::new(RefCell::new(1));
    let render = {
        let (events, received, key) = (events.clone(), Rc::clone(&received), Rc::clone(&key));
        move |composition: &mut Composition<MemoryApplier>| {
            let (events, received, key) = (events.clone(), Rc::clone(&received), *key.borrow());
            composition
                .render(1, move || {
                    let received = Rc::clone(&received);
                    CollectFlow(
                        key,
                        events.as_shared_flow().map(move |event: u32| (key, event)),
                        move |item| {
                            received.borrow_mut().push(item);
                        },
                    );
                })
                .expect("render");
        }
    };
    let start = render.clone();
    start(&mut composition);
    composition.runtime_handle().drain_ui();
    events.emit(10);
    assert!(pump_until(&mut composition, render.clone(), || received
        .borrow()
        .len()
        == 1));
    *key.borrow_mut() = 2;
    let rekey = render.clone();
    rekey(&mut composition);
    composition.runtime_handle().drain_ui();
    events.emit(20);
    assert!(pump_until(&mut composition, render, || received
        .borrow()
        .len()
        == 2));
    assert_eq!(*received.borrow(), vec![(1, 10), (2, 20)]);
    assert_eq!(
        events.subscription_count(),
        1,
        "the old collection was cancelled"
    );
}

type Render = Rc<dyn Fn(&mut Composition<MemoryApplier>)>;

struct SnapshotHarness<S: Copy + 'static> {
    composition: Composition<MemoryApplier>,
    source: Rc<RefCell<Option<S>>>,
    received: Rc<RefCell<Vec<String>>>,
    render: Render,
}

impl<S: Copy + 'static> SnapshotHarness<S> {
    fn start(create: fn() -> S, read: fn(S) -> String) -> Self {
        let source: Rc<RefCell<Option<S>>> = Rc::default();
        let received: Rc<RefCell<Vec<String>>> = Rc::default();
        let render: Render = {
            let (source, received) = (Rc::clone(&source), Rc::clone(&received));
            Rc::new(move |composition: &mut Composition<MemoryApplier>| {
                let (source, received) = (Rc::clone(&source), Rc::clone(&received));
                composition
                    .render(1, move || {
                        let state = remember(create).with(|state| *state);
                        *source.borrow_mut() = Some(state);
                        let received = Rc::clone(&received);
                        CollectFlow((), snapshotFlow(move || read(state)), move |value| {
                            received.borrow_mut().push(value);
                        });
                    })
                    .expect("render");
            })
        };
        let mut harness = Self {
            composition: composition(),
            source,
            received,
            render,
        };
        (harness.render)(&mut harness.composition);
        assert!(harness.receives(1, PATIENCE), "no first value");
        harness
    }

    fn write(&self, write: impl FnOnce(S)) {
        let state = *self.source.borrow();
        if let Some(state) = state {
            write(state);
        }
    }

    fn receives(&mut self, count: usize, patience: Duration) -> bool {
        let render = Rc::clone(&self.render);
        let received = Rc::clone(&self.received);
        pump_for(
            patience,
            &mut self.composition,
            move |composition| render(composition),
            move || received.borrow().len() == count,
        )
    }

    fn received(&self) -> Vec<String> {
        self.received.borrow().clone()
    }
}

#[test]
fn snapshot_flow_emits_when_the_state_it_reads_changes() {
    let mut harness =
        SnapshotHarness::start(|| mutableStateOf(String::new()), |state| state.value());
    harness.write(|state| state.set("rust".to_string()));
    assert!(harness.receives(2, PATIENCE));
    harness.write(|state| state.set("rust".to_string()));
    assert!(
        !harness.receives(3, QUIET_PERIOD),
        "an equal value is not emitted again"
    );
    assert_eq!(harness.received(), vec![String::new(), "rust".to_string()]);
}

#[test]
fn snapshot_flow_sees_writes_made_inside_an_event_handler_snapshot() {
    let mut harness = SnapshotHarness::start(
        || mutableStateOf(String::new()),
        |state| state.with(String::clone),
    );
    harness.write(|state| {
        let _ = cranpose_core::run_in_mutable_snapshot(|| state.set("typed".to_string()));
    });
    assert!(
        harness.receives(2, PATIENCE),
        "received {:?}",
        harness.received()
    );
}

#[test]
fn snapshot_flow_sees_text_field_edits() {
    use cranpose_foundation::text::TextFieldState;
    let mut harness = SnapshotHarness::start(|| TextFieldState::new(""), |state| state.text());
    harness.write(|state| {
        let _ = cranpose_core::run_in_mutable_snapshot(|| state.set_text("typed"));
    });
    assert!(
        harness.receives(2, PATIENCE),
        "received {:?}",
        harness.received()
    );
}
