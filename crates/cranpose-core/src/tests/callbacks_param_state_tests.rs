use std::{cell::Cell, rc::Rc, sync::Arc};

use super::ParamState;

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
        state.update_shared(&Rc::clone(&shared)),
        "the first value is new"
    );
    assert!(!state.update_shared(&Rc::clone(&shared)));
    assert_eq!(comparisons.get(), 0, "one allocation needs no comparison");

    assert!(
        !state.update_shared(&Rc::new(counted(1))),
        "an equal value in another allocation is unchanged"
    );
    assert_eq!(comparisons.get(), 1);
    assert!(state.update_shared(&Rc::new(counted(2))));
    assert_eq!(comparisons.get(), 2);
}

#[test]
fn an_arc_parameter_takes_the_same_shortcut() {
    let shared = Arc::new(vec![1, 2, 3]);
    let mut state = ParamState::default();
    assert!(state.update_shared(&Arc::clone(&shared)));
    assert!(!state.update_shared(&Arc::clone(&shared)));
    assert!(!state.update_shared(&Arc::new(vec![1, 2, 3])));
    assert!(state.update_shared(&Arc::new(vec![4])));
}
