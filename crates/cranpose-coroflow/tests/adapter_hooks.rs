use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

mod support;

use coroflow::{FlowExt, MainScope, MutableSharedFlow};
use cranpose_core::{Composition, MemoryApplier};
use cranpose_coroflow::{FlowCollect, Handle, rememberCoroutineScope};
use support::{composition, pump_until};

struct DropMarker(Arc<AtomicUsize>);

impl Drop for DropMarker {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn any_flow_collects_into_state_from_an_initial_value() {
    let mut composition = composition();
    let events = MutableSharedFlow::new(0, 4);
    let flow = events.as_shared_flow().map(|value: u32| value * 10);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let render = {
        let seen = Rc::clone(&seen);
        move |composition: &mut Composition<MemoryApplier>| {
            let (seen, flow) = (Rc::clone(&seen), flow.clone());
            composition
                .render(1, move || {
                    seen.borrow_mut().push(flow.collectAsState(7).get());
                })
                .expect("render");
        }
    };
    let subscribed = events.clone();
    assert!(pump_until(&mut composition, render.clone(), move || {
        subscribed.subscription_count().value() == 1
    }));
    assert_eq!(
        seen.borrow().first(),
        Some(&7),
        "starts from the initial value"
    );
    assert!(events.try_emit(5));
    let latest = Rc::clone(&seen);
    assert!(pump_until(&mut composition, render, move || {
        latest.borrow().last() == Some(&50)
    }));
}

#[test]
fn remember_coroutine_scope_is_cancelled_when_its_position_leaves() {
    let mut composition = composition();
    let show = Rc::new(RefCell::new(true));
    let remembered: Rc<RefCell<Option<Handle<MainScope>>>> = Rc::default();
    let render = {
        let (show, remembered) = (Rc::clone(&show), Rc::clone(&remembered));
        move |composition: &mut Composition<MemoryApplier>| {
            let (show, remembered) = (Rc::clone(&show), Rc::clone(&remembered));
            composition
                .render(1, move || {
                    if *show.borrow() {
                        *remembered.borrow_mut() = Some(rememberCoroutineScope());
                    }
                })
                .expect("render");
        }
    };
    let first = render.clone();
    first(&mut composition);
    let cancelled = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(AtomicUsize::new(0));
    if let Some(scope) = *remembered.borrow() {
        let (cancelled, started) = (Arc::clone(&cancelled), Arc::clone(&started));
        scope.get().launch(async move {
            let _marker = DropMarker(cancelled);
            started.fetch_add(1, Ordering::SeqCst);
            std::future::pending::<()>().await;
        });
    }
    let running = Arc::clone(&started);
    assert!(pump_until(&mut composition, render.clone(), move || {
        running.load(Ordering::SeqCst) == 1
    }));
    *show.borrow_mut() = false;
    remembered.borrow_mut().take();
    let gone = Arc::clone(&cancelled);
    assert!(pump_until(&mut composition, render, move || {
        gone.load(Ordering::SeqCst) == 1
    }));
}
