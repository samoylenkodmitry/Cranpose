use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

mod support;

use coroflow::{MainScope, MutableStateFlow, StateFlow};
use cranpose_core::{Composition, MemoryApplier};
use cranpose_coroflow::{
    Handle, ProvideViewModelStore, StateFlowCollect, ViewModelStore, ViewModelStoreOwner, viewModel,
};
use support::{DropMarker, composition, pump_until};

struct CounterViewModel {
    scope: MainScope,
    count: MutableStateFlow<u32>,
}

impl CounterViewModel {
    fn new(scope: MainScope, dropped: &Arc<AtomicUsize>) -> Self {
        let dropped = Arc::clone(dropped);
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

struct Labelled(&'static str);

type Handles<T> = Rc<RefCell<Vec<Handle<T>>>>;

type Resolved<T> = Rc<RefCell<Vec<(Handle<T>, Rc<T>)>>>;

fn render(composition: &mut Composition<MemoryApplier>, content: impl FnMut() + 'static) {
    composition.render(1, content).expect("render");
    composition.runtime_handle().drain_ui();
}

#[test]
fn outside_any_store_a_view_model_lives_as_long_as_its_position() {
    let mut composition = composition();
    let dropped = Arc::new(AtomicUsize::new(0));
    let models: Handles<CounterViewModel> = Rc::default();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let frame = {
        let (dropped, models, seen) = (Arc::clone(&dropped), Rc::clone(&models), Rc::clone(&seen));
        move |composition: &mut Composition<MemoryApplier>| {
            let (dropped, models, seen) =
                (Arc::clone(&dropped), Rc::clone(&models), Rc::clone(&seen));
            render(composition, move || {
                let model = viewModel((), |scope| CounterViewModel::new(scope, &dropped));
                seen.borrow_mut()
                    .push(model.get().count().collectAsState().get());
                models.borrow_mut().push(model);
            });
        }
    };
    frame.clone()(&mut composition);
    frame.clone()(&mut composition);
    {
        let models = models.borrow();
        assert!(models[0] == models[1], "one view model per position");
        assert!(Rc::ptr_eq(&models[0].get(), &models[1].get()));
        models[0].get().increment();
    }
    assert!(pump_until(&mut composition, frame, || seen.borrow().last() == Some(&1)));
    models.borrow_mut().clear();

    render(&mut composition, || {});
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        1,
        "leaving the composition drops the view model and cancels its scope"
    );
}

#[test]
fn a_stored_view_model_outlives_its_caller_and_comes_back() {
    let mut composition = composition();
    let store = ViewModelStore::default();
    let show = Rc::new(Cell::new(true));
    let created = Rc::new(Cell::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let models: Rc<RefCell<Vec<Rc<CounterViewModel>>>> = Rc::default();
    let frame = |composition: &mut Composition<MemoryApplier>| {
        let (store, show, created, dropped, models) = (
            store.clone(),
            Rc::clone(&show),
            Rc::clone(&created),
            Arc::clone(&dropped),
            Rc::clone(&models),
        );
        render(composition, move || {
            let (show, created, dropped, models) = (
                Rc::clone(&show),
                Rc::clone(&created),
                Arc::clone(&dropped),
                Rc::clone(&models),
            );
            ProvideViewModelStore(store.clone(), move || {
                if show.get() {
                    let model = viewModel((), |scope| {
                        created.set(created.get() + 1);
                        CounterViewModel::new(scope, &dropped)
                    });
                    models.borrow_mut().push(model.get());
                }
            });
        });
    };
    frame(&mut composition);
    show.set(false);
    frame(&mut composition);
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        0,
        "the store keeps the view model"
    );
    show.set(true);
    frame(&mut composition);
    assert_eq!(created.get(), 1, "the caller gets its view model back");
    let models = models.borrow();
    assert!(Rc::ptr_eq(&models[0], &models[1]));
}

#[test]
fn keys_tell_view_models_of_one_type_apart() {
    let mut composition = composition();
    let store = ViewModelStore::default();
    let models: Handles<Labelled> = Rc::default();
    let sink = Rc::clone(&models);
    render(&mut composition, move || {
        let sink = Rc::clone(&sink);
        ProvideViewModelStore(store.clone(), move || {
            let first = viewModel(1_u32, |_| Labelled("first"));
            let second = viewModel(2_u32, |_| Labelled("second"));
            let first_again = viewModel(1_u32, |_| Labelled("a copy"));
            sink.borrow_mut().extend([first, second, first_again]);
        });
    });
    let labels: Vec<_> = models.borrow().iter().map(|model| model.get().0).collect();
    assert_eq!(labels, ["first", "second", "first"]);
}

#[test]
fn a_new_view_model_at_a_position_gives_a_new_handle() {
    let mut composition = composition();
    let store = ViewModelStore::default();
    let key = Rc::new(Cell::new(1_u32));
    let models: Resolved<Labelled> = Rc::default();
    let frame = |composition: &mut Composition<MemoryApplier>| {
        let (store, key, models) = (store.clone(), Rc::clone(&key), Rc::clone(&models));
        render(composition, move || {
            let (key, models) = (key.get(), Rc::clone(&models));
            ProvideViewModelStore(store.clone(), move || {
                let label = if key == 1 { "one" } else { "two" };
                let model = viewModel(key, |_| Labelled(label));
                models.borrow_mut().push((model, model.get()));
            });
        });
    };
    frame(&mut composition);
    key.set(2);
    frame(&mut composition);
    key.set(1);
    frame(&mut composition);
    let models = models.borrow();
    assert!(
        models[0].0 != models[1].0,
        "a different view model, a different handle"
    );
    assert_eq!(models[1].1.0, "two");
    assert!(
        Rc::ptr_eq(&models[0].1, &models[2].1),
        "the first key's view model is still in the store"
    );
}

#[test]
fn a_view_model_store_owner_drops_its_view_models_when_it_leaves() {
    let mut composition = composition();
    let show = Rc::new(Cell::new(true));
    let dropped = Arc::new(AtomicUsize::new(0));
    let frame = |composition: &mut Composition<MemoryApplier>| {
        let (show, dropped) = (Rc::clone(&show), Arc::clone(&dropped));
        render(composition, move || {
            if show.get() {
                let dropped = Arc::clone(&dropped);
                ViewModelStoreOwner(move || {
                    viewModel((), |scope| CounterViewModel::new(scope, &dropped));
                });
            }
        });
    };
    frame(&mut composition);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    show.set(false);
    frame(&mut composition);
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        1,
        "the block's view models end with the block"
    );
}

#[test]
fn a_seeded_store_hands_the_screen_its_view_model() {
    let mut composition = composition();
    let store = ViewModelStore::default();
    store.put((), Labelled("fake"));
    let built = Rc::new(Cell::new(false));
    let label = Rc::new(Cell::new(""));
    let (flag, sink) = (Rc::clone(&built), Rc::clone(&label));
    render(&mut composition, move || {
        let (flag, sink) = (Rc::clone(&flag), Rc::clone(&sink));
        ProvideViewModelStore(store.clone(), move || {
            let model = viewModel((), |_| {
                flag.set(true);
                Labelled("real")
            });
            sink.set(model.get().0);
        });
    });
    assert!(!built.get(), "the factory is not called");
    assert_eq!(label.get(), "fake");
}

#[test]
fn a_store_gets_replaces_and_clears_its_view_models() {
    let store = ViewModelStore::default();
    let dropped = Arc::new(AtomicUsize::new(0));
    drop(store.put("a", DropMarker(Arc::clone(&dropped))));
    assert!(store.get::<&str, DropMarker>(&"a").is_some());
    assert!(store.get::<&str, DropMarker>(&"b").is_none());
    assert!(
        store.get::<u32, DropMarker>(&0).is_none(),
        "a key of another type is another slot"
    );
    drop(store.put("a", DropMarker(Arc::clone(&dropped))));
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        1,
        "put drops the model it replaces"
    );
    let shared = store.clone();
    assert!(shared == store, "clones share one store");
    assert!(ViewModelStore::default() != store);
    shared.clear();
    assert_eq!(dropped.load(Ordering::SeqCst), 2, "clear drops every model");
    assert!(store.get::<&str, DropMarker>(&"a").is_none());
}
