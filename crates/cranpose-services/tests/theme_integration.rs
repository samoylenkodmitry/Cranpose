use std::{cell::RefCell, rc::Rc};

use cranpose_core::{Composition, MemoryApplier, location_key};
use cranpose_services::{ProvideSystemTheme, SystemTheme, isSystemInDarkTheme};

fn run_test_composition(build: impl FnMut()) {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), build)
        .expect("initial render succeeds");
}

#[test]
fn integration_is_system_in_dark_theme_respects_provider() {
    let captured = Rc::new(RefCell::new(None));

    {
        let captured = Rc::clone(&captured);
        run_test_composition(move || {
            let captured = Rc::clone(&captured);
            ProvideSystemTheme(SystemTheme::Dark, move || {
                *captured.borrow_mut() = Some(isSystemInDarkTheme());
            });
        });
    }

    assert_eq!(*captured.borrow(), Some(true));
}
