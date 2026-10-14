use std::{cell::RefCell, rc::Rc};

use cranpose_core::{Composition, MemoryApplier, location_key};
use cranpose_services::{
    ProvideSystemTheme, SystemTheme, clear_platform_system_theme, default_system_theme,
    isSystemInDarkTheme, known_system_theme, set_platform_system_theme,
};

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

#[test]
fn integration_known_system_theme_never_detects() {
    assert_eq!(known_system_theme(), None);

    set_platform_system_theme(SystemTheme::Dark);
    assert_eq!(known_system_theme(), Some(SystemTheme::Dark));

    clear_platform_system_theme();
    assert_eq!(known_system_theme(), None);

    let detected = default_system_theme();
    assert_eq!(known_system_theme(), Some(detected));
}
