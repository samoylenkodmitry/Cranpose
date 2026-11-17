// The scopes a pass recomposes in the slots of a subcompose layout run slot
// by slot, in the order each slot's first scope came in the pass, and the
// scopes of one slot in the order their states changed. A slot whose scopes
// come between another's runs once.

use std::{cell::RefCell, rc::Rc};

use cranpose_core::{Composition, MemoryApplier, MutableState, SlotId, location_key};
use cranpose_ui::{
    AppContext, LayoutEngine, MeasureResult, Modifier, Size, SubcomposeMeasureScope,
};

type Log = Rc<RefCell<Vec<(usize, usize)>>>;

const READERS: usize = 3;

#[cranpose_ui::composable]
fn Reader(slot: usize, index: usize, state: MutableState<u32>, log: Log) {
    let _: u32 = state.value();
    log.borrow_mut().push((slot, index));
}

fn render(
    context: &Rc<AppContext>,
    composition: &mut Composition<MemoryApplier>,
    states: &Rc<[[MutableState<u32>; READERS]; 2]>,
    log: &Log,
) {
    let (states, log) = (Rc::clone(states), Rc::clone(log));
    context
        .enter(|| {
            composition.render(location_key(file!(), line!(), column!()), move || {
                let (states, log) = (Rc::clone(&states), Rc::clone(&log));
                cranpose_ui::SubcomposeLayout(Modifier::empty(), move |scope, constraints| {
                    for slot in 0..2 {
                        let (states, log) = (Rc::clone(&states), Rc::clone(&log));
                        scope.subcompose(SlotId::new(slot as u64), (), move || {
                            for (index, &state) in states[slot].iter().enumerate() {
                                cranpose_core::with_key(&index, || {
                                    Reader(slot, index, state, Rc::clone(&log));
                                });
                            }
                        });
                    }
                    let (width, height) = constraints.constrain(0.0, 0.0);
                    MeasureResult::new(Size::new(width, height), Vec::new())
                });
            })
        })
        .expect("the composition");
    context.enter(|| {
        let root = composition.root().expect("the subcompose layout");
        let runtime = composition.runtime_handle();
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(runtime);
        applier
            .compute_layout(root, Size::new(100.0, 100.0))
            .expect("the layout");
        applier.clear_runtime_handle();
    });
}

#[test]
fn scopes_of_two_slots_recompose_slot_by_slot_in_the_order_they_came() {
    let context = AppContext::new();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let states: Rc<[[MutableState<u32>; READERS]; 2]> = Rc::new(std::array::from_fn(|_| {
        std::array::from_fn(|_| MutableState::with_runtime(0, runtime.clone()))
    }));
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    render(&context, &mut composition, &states, &log);
    assert_eq!(log.borrow().len(), 2 * READERS, "every reader composed");

    for round in 1..4u32 {
        log.borrow_mut().clear();
        // Slot 1 first, then the slots by turns: 1.2, 0.1, 1.0, 0.2, 0.0.
        for (slot, index) in [(1, 2), (0, 1), (1, 0), (0, 2), (0, 0)] {
            states[slot][index].set(round);
        }
        context.enter(|| {
            composition
                .process_invalid_scopes()
                .expect("the readers recompose");
        });
        assert_eq!(
            *log.borrow(),
            [(1, 2), (1, 0), (0, 1), (0, 2), (0, 0)],
            "round {round}"
        );
    }
}
