use cranpose_animation::{
    infiniteRepeatable, rememberInfiniteTransition, AnimationSpec, RepeatMode, StartOffset,
};
use cranpose_core::{location_key, with_current_composer, Composition, MemoryApplier, State};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn infinite_transition_drives_state_updates() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let root_key = location_key(file!(), line!(), column!());
    let group_key = location_key(file!(), line!(), column!());
    let state_slot = Rc::new(RefCell::new(None::<State<f32>>));

    {
        let state_slot = Rc::clone(&state_slot);
        composition
            .render(root_key, move || {
                let state_slot = Rc::clone(&state_slot);
                with_current_composer(|composer| {
                    composer.with_group(group_key, |_| {
                        let transition = rememberInfiniteTransition("integration_pulse");
                        let state = transition.animateFloat(
                            0.0,
                            1.0,
                            infiniteRepeatable(
                                AnimationSpec::linear(800),
                                RepeatMode::Reverse,
                                StartOffset::default(),
                            ),
                            "integration_pulse",
                        );
                        state_slot.borrow_mut().replace(state);
                    });
                });
            })
            .expect("initial render");
    }

    let initial = state_slot.borrow().as_ref().expect("state available").get();
    assert_eq!(initial, 0.0);

    let mut time = 0u64;
    let mut saw_change = false;
    for _ in 0..40 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        let _ = composition
            .process_invalid_scopes()
            .expect("process invalid scopes succeeds");
        let value = state_slot.borrow().as_ref().expect("state available").get();
        if (value - initial).abs() > 0.0001 {
            saw_change = true;
            break;
        }
    }

    assert!(saw_change, "state should change as frames advance");
}
