use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

mod support;

use cranpose_core::{Composition, MemoryApplier};
use cranpose_coroflow::{LifecycleResumeEffect, LifecycleStartEffect};
use cranpose_services::{LifecycleState, dispatch_lifecycle_state};
use support::{composition, pump_until};

type Log = Rc<RefCell<Vec<&'static str>>>;

fn take(log: &Log) -> Vec<&'static str> {
    std::mem::take(&mut *log.borrow_mut())
}

// One test per binary: the host lifecycle is process-wide.
#[test]
fn lifecycle_effects_follow_the_host_and_their_keys() {
    dispatch_lifecycle_state(LifecycleState::Started);
    dispatch_lifecycle_state(LifecycleState::Resumed);
    let mut composition = composition();
    let log: Log = Rc::default();
    let key = Rc::new(Cell::new(0));
    let show = Rc::new(Cell::new(true));
    let frame = {
        let (log, key, show) = (Rc::clone(&log), Rc::clone(&key), Rc::clone(&show));
        move |composition: &mut Composition<MemoryApplier>| {
            let (log, key, show) = (Rc::clone(&log), key.get(), show.get());
            composition
                .render(1, move || {
                    if !show {
                        return;
                    }
                    let (started, stopped) = (Rc::clone(&log), Rc::clone(&log));
                    LifecycleStartEffect(key, move |scope| {
                        started.borrow_mut().push("start");
                        scope.on_stop_or_dispose(move || stopped.borrow_mut().push("stop"))
                    });
                    let (resumed, paused) = (Rc::clone(&log), Rc::clone(&log));
                    LifecycleResumeEffect(key, move |scope| {
                        resumed.borrow_mut().push("resume");
                        scope.on_pause_or_dispose(move || paused.borrow_mut().push("pause"))
                    });
                })
                .expect("render");
        }
    };
    let expect = |composition: &mut Composition<MemoryApplier>, wanted: &[&'static str]| {
        let seen = Rc::clone(&log);
        let count = wanted.len();
        assert!(
            pump_until(composition, frame.clone(), move || seen.borrow().len()
                >= count),
            "waited for {wanted:?}"
        );
        let mut got = take(&log);
        got.sort_unstable();
        let mut wanted = wanted.to_vec();
        wanted.sort_unstable();
        assert_eq!(got, wanted);
    };

    expect(&mut composition, &["start", "resume"]);
    dispatch_lifecycle_state(LifecycleState::Paused);
    expect(&mut composition, &["pause"]);
    dispatch_lifecycle_state(LifecycleState::Stopped);
    expect(&mut composition, &["stop"]);
    dispatch_lifecycle_state(LifecycleState::Started);
    expect(&mut composition, &["start"]);
    dispatch_lifecycle_state(LifecycleState::Resumed);
    expect(&mut composition, &["resume"]);
    key.set(1);
    expect(&mut composition, &["stop", "start", "pause", "resume"]);
    show.set(false);
    expect(&mut composition, &["stop", "pause"]);
}
