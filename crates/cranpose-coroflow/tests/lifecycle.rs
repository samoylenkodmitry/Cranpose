mod support;

use std::{cell::RefCell, rc::Rc};

use coroflow::MutableStateFlow;
use cranpose_core::{Composition, MemoryApplier};
use cranpose_coroflow::{StateFlowCollect, collects_in};
use cranpose_services::{LifecycleState, dispatch_lifecycle_state};
use support::{composition, pump_until};

#[test]
fn only_stopped_or_destroyed_hosts_pause_collection() {
    assert!(
        collects_in(LifecycleState::Created),
        "hosts without lifecycle"
    );
    assert!(collects_in(LifecycleState::Started));
    assert!(collects_in(LifecycleState::Resumed));
    assert!(
        collects_in(LifecycleState::Paused),
        "paused is still started"
    );
    assert!(!collects_in(LifecycleState::Stopped));
    assert!(!collects_in(LifecycleState::Destroyed));
}

#[test]
fn a_lifecycle_aware_collection_lets_go_while_the_app_is_stopped() {
    let mut composition = composition();
    let source = MutableStateFlow::new(1);
    let flow = source.as_state_flow();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let render = {
        let seen = Rc::clone(&seen);
        move |composition: &mut Composition<MemoryApplier>| {
            let (seen, flow) = (Rc::clone(&seen), flow.clone());
            composition
                .render(1, move || {
                    seen.borrow_mut()
                        .push(flow.collectAsStateWithLifecycle().get());
                })
                .expect("render");
        }
    };
    dispatch_lifecycle_state(LifecycleState::Started);
    dispatch_lifecycle_state(LifecycleState::Resumed);
    let collecting = source.clone();
    assert!(pump_until(&mut composition, render.clone(), move || {
        collecting.subscription_count().value() == 1
    }));

    dispatch_lifecycle_state(LifecycleState::Paused);
    dispatch_lifecycle_state(LifecycleState::Stopped);
    let released = source.clone();
    assert!(
        pump_until(&mut composition, render.clone(), move || {
            released.subscription_count().value() == 0
        }),
        "a stopped app is not a subscriber"
    );
    source.set(5);
    let last_while_stopped = seen.borrow().last().copied();
    assert_eq!(
        last_while_stopped,
        Some(1),
        "the state keeps the last value"
    );

    dispatch_lifecycle_state(LifecycleState::Started);
    let back = Rc::clone(&seen);
    assert!(pump_until(&mut composition, render, move || {
        back.borrow().last() == Some(&5)
    }));
    assert_eq!(source.subscription_count().value(), 1);
}
