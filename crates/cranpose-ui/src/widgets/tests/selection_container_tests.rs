use cranpose_core::MutableState;

use super::*;
use crate::{run_test_composition, widgets::Text};

type Seen = Rc<RefCell<Option<SelectionRegistrar>>>;

/// Records the registrar the texts composed here select in.
fn remember_registrar(seen: &Seen) {
    *seen.borrow_mut() = local_selection_registrar().current();
}

fn registrar(seen: &Seen) -> SelectionRegistrar {
    seen.borrow()
        .clone()
        .expect("the container provides a registrar")
}

#[test]
fn the_text_in_a_container_joins_it_and_disabled_text_stays_out() {
    let _app_context = crate::render_state::app_context_test_scope();
    let seen: Seen = Rc::new(RefCell::new(None));
    let outside: Seen = Rc::new(RefCell::new(None));
    let composition = {
        let (seen, outside) = (Rc::clone(&seen), Rc::clone(&outside));
        run_test_composition(move || {
            remember_registrar(&outside);
            let seen = Rc::clone(&seen);
            SelectionContainer(Modifier::empty(), move || {
                remember_registrar(&seen);
                Text("kept", Modifier::empty(), TextStyle::default());
                DisableSelection(|| {
                    Text("left out", Modifier::empty(), TextStyle::default());
                });
                Text("also kept", Modifier::empty(), TextStyle::default());
            });
        })
    };

    assert!(
        outside.borrow().is_none(),
        "text outside a container is not selectable"
    );
    let registrar = registrar(&seen);
    composition.with_app_context(|| {
        registrar.select_all();
        assert_eq!(registrar.selected_text(), "kept\nalso kept");
    });
}

#[test]
fn a_text_that_leaves_the_container_leaves_its_selection() {
    let _app_context = crate::render_state::app_context_test_scope();
    let seen: Seen = Rc::new(RefCell::new(None));
    let shown: Rc<RefCell<Option<MutableState<bool>>>> = Rc::new(RefCell::new(None));
    let mut composition = {
        let (seen, shown) = (Rc::clone(&seen), Rc::clone(&shown));
        run_test_composition(move || {
            let seen = Rc::clone(&seen);
            let shown = Rc::clone(&shown);
            SelectionContainer(Modifier::empty(), move || {
                remember_registrar(&seen);
                let visible = cranpose_core::rememberMutableStateOf(|| true);
                *shown.borrow_mut() = Some(visible);
                Text("stays", Modifier::empty(), TextStyle::default());
                if visible.get() {
                    Text("goes", Modifier::empty(), TextStyle::default());
                }
            });
        })
    };
    let registrar = registrar(&seen);
    composition.with_app_context(|| registrar.select_all());
    assert_eq!(
        composition.with_app_context(|| registrar.selected_text()),
        "stays\ngoes"
    );

    let visible = shown.borrow().expect("the toggle was composed");
    visible.set(false);
    composition
        .process_invalid_scopes()
        .expect("recomposition succeeds");

    assert_eq!(
        registrar.selection(),
        None,
        "the selection ended in the text that left"
    );
    composition.with_app_context(|| {
        registrar.select_all();
        assert_eq!(registrar.selected_text(), "stays");
    });
}
