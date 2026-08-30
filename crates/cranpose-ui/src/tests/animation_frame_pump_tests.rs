use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_animation::{Animatable, AnimationSpec, AnimationType};
use cranpose_core::with_current_composer;
use cranpose_macros::composable;

use crate::{TestComposition, run_test_composition};

fn drain(composition: &mut TestComposition) {
    while composition
        .process_invalid_scopes()
        .expect("process invalid scopes")
    {}
}

fn update_at(composition: &mut TestComposition, frame_time: u64) {
    let runtime = composition.runtime_handle();
    composition.with_app_context(|| {
        runtime.with_deferred_state_releases(|| {
            runtime.drain_frame_callbacks(frame_time);
            runtime.drain_ui();
        });
    });
    drain(composition);
}

impl PartialEq for PumpProbe {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

struct PumpProbe {
    gate: RefCell<Animatable<f32>>,
    tail: RefCell<Animatable<f32>>,
    composes: Cell<u32>,
    observed_gate: Cell<f32>,
    observed_tail: Cell<f32>,
}

#[composable]
#[allow(non_snake_case)]
fn PumpHost(probe: Rc<PumpProbe>) {
    probe.composes.set(probe.composes.get() + 1);
    probe.observed_gate.set(probe.gate.borrow().state().value());
    probe.observed_tail.set(probe.tail.borrow().state().value());
}

#[test]
fn frame_pump_survives_exact_boundary_completions() {
    let probe_slot: Rc<RefCell<Option<Rc<PumpProbe>>>> = Rc::new(RefCell::new(None));
    let probe_for_host = Rc::clone(&probe_slot);
    let mut composition = run_test_composition(move || {
        let probe = cranpose_core::remember(|| {
            let runtime = with_current_composer(|composer| composer.runtime_handle());
            Rc::new(PumpProbe {
                gate: RefCell::new(Animatable::new(0.0, runtime.clone())),
                tail: RefCell::new(Animatable::new(0.0, runtime)),
                composes: Cell::new(0),
                observed_gate: Cell::new(0.0),
                observed_tail: Cell::new(0.0),
            })
        })
        .with(Rc::clone);
        *probe_for_host.borrow_mut() = Some(Rc::clone(&probe));
        PumpHost(probe);
    });
    drain(&mut composition);
    let probe = probe_slot.borrow().clone().expect("probe captured");

    let ms = |v: u64| v * 1_000_000;
    composition.with_app_context(|| {
        probe
            .gate
            .borrow_mut()
            .animateTo(1.0, AnimationType::Tween(AnimationSpec::linear(120)));
        probe
            .tail
            .borrow_mut()
            .animateTo(1.0, AnimationType::Tween(AnimationSpec::linear(70)));
    });

    update_at(&mut composition, ms(10));
    update_at(&mut composition, ms(70));
    assert!((probe.observed_gate.get() - 0.5).abs() < 1e-3);

    update_at(&mut composition, ms(130));
    assert_eq!(
        probe.observed_gate.get(),
        1.0,
        "the completion write must recompose the subscribed scope"
    );
    assert_eq!(probe.observed_tail.get(), 1.0);

    let composes_before = probe.composes.get();
    composition.with_app_context(|| {
        probe
            .tail
            .borrow_mut()
            .animateTo(0.0, AnimationType::Tween(AnimationSpec::linear(50)));
    });
    update_at(&mut composition, ms(140));
    update_at(&mut composition, ms(165));
    assert!(
        probe.composes.get() > composes_before,
        "recomposition died after the boundary-completion frame"
    );
    assert!(
        (probe.observed_tail.get() - 0.5).abs() < 0.02,
        "the post-boundary animation must actually animate, got {}",
        probe.observed_tail.get()
    );
    update_at(&mut composition, ms(200));
    assert_eq!(probe.observed_tail.get(), 0.0);
}
