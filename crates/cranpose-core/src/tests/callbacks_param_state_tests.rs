use std::{cell::Cell, rc::Rc, sync::Arc};

use super::{ParamState, SharedParam, refresh_param, refresh_shared_param};

fn update_shared<T: SharedParam + PartialEq + Clone>(state: &mut ParamState<T>, new: &T) -> bool {
    state.update_fields(|| new.clone(), |stored| refresh_shared_param(stored, new))
}

/// A value that counts how often it is compared.
#[derive(Clone)]
struct Counted {
    value: u32,
    comparisons: Rc<Cell<usize>>,
}

impl PartialEq for Counted {
    fn eq(&self, other: &Self) -> bool {
        self.comparisons.set(self.comparisons.get() + 1);
        self.value == other.value
    }
}

#[test]
fn the_same_allocation_is_unchanged_without_comparing_its_contents() {
    let comparisons = Rc::new(Cell::new(0));
    let counted = |value| Counted {
        value,
        comparisons: Rc::clone(&comparisons),
    };
    let shared = Rc::new(counted(1));
    let mut state = ParamState::default();

    assert!(
        update_shared(&mut state, &Rc::clone(&shared)),
        "the first value is new"
    );
    assert!(!update_shared(&mut state, &Rc::clone(&shared)));
    assert_eq!(comparisons.get(), 0, "one allocation needs no comparison");

    assert!(
        !update_shared(&mut state, &Rc::new(counted(1))),
        "an equal value in another allocation is unchanged"
    );
    assert_eq!(comparisons.get(), 1);
    assert!(update_shared(&mut state, &Rc::new(counted(2))));
    assert_eq!(comparisons.get(), 2);
}

#[test]
fn an_arc_parameter_takes_the_same_shortcut() {
    let shared = Arc::new(vec![1, 2, 3]);
    let mut state = ParamState::default();
    assert!(update_shared(&mut state, &Arc::clone(&shared)));
    assert!(!update_shared(&mut state, &Arc::clone(&shared)));
    assert!(!update_shared(&mut state, &Arc::new(vec![1, 2, 3])));
    assert!(update_shared(&mut state, &Arc::new(vec![4])));
}

/// A value that counts how often it is cloned.
#[derive(PartialEq)]
struct Cloned {
    value: u32,
    clones: Rc<Cell<usize>>,
}

impl Clone for Cloned {
    fn clone(&self) -> Self {
        self.clones.set(self.clones.get() + 1);
        Self {
            value: self.value,
            clones: Rc::clone(&self.clones),
        }
    }
}

#[test]
fn a_call_stores_its_parameters_once_and_clones_only_the_ones_that_changed() {
    let clones = Rc::new(Cell::new(0));
    let cloned = |value| Cloned {
        value,
        clones: Rc::clone(&clones),
    };
    let mut state = ParamState::<(Cloned, Cloned)>::default();
    let update = |state: &mut ParamState<(Cloned, Cloned)>, first: &Cloned, second: &Cloned| {
        state.update_fields(
            || (first.clone(), second.clone()),
            |stored| refresh_param(&mut stored.0, first) | refresh_param(&mut stored.1, second),
        )
    };

    assert!(
        update(&mut state, &cloned(1), &cloned(2)),
        "the first call is new"
    );
    assert_eq!(clones.get(), 2);
    assert!(!update(&mut state, &cloned(1), &cloned(2)));
    assert_eq!(clones.get(), 2, "unchanged parameters are not cloned again");
    assert!(update(&mut state, &cloned(1), &cloned(3)));
    assert_eq!(clones.get(), 3, "only the changed parameter is cloned");
    let stored = state.value().expect("stored parameters");
    assert_eq!((stored.0.value, stored.1.value), (1, 3));
}
