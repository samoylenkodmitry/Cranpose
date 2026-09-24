use std::{cell::RefCell, rc::Rc};

use cranpose_core::CompositionLocalProvider;

use super::*;
use crate::run_test_composition;

#[test]
fn default_system_theme_returns_supported_variant() {
    assert!(matches!(
        default_system_theme(),
        SystemTheme::Light | SystemTheme::Dark
    ));
}

#[test]
fn platform_pushed_theme_wins_over_detection() {
    clear_platform_system_theme();
    set_platform_system_theme(SystemTheme::Dark);
    assert_eq!(default_system_theme(), SystemTheme::Dark);
    set_platform_system_theme(SystemTheme::Light);
    assert_eq!(default_system_theme(), SystemTheme::Light);
    clear_platform_system_theme();
}

#[test]
fn local_system_theme_can_be_overridden() {
    let local = local_system_theme();
    let captured = Rc::new(RefCell::new(None));

    {
        let captured = Rc::clone(&captured);
        let local_for_provider = local.clone();
        run_test_composition(move || {
            let captured = Rc::clone(&captured);
            let local = local.clone();
            CompositionLocalProvider(
                vec![local_for_provider.provides(SystemTheme::Dark)],
                move || {
                    *captured.borrow_mut() = Some(local.current());
                },
            );
        });
    }

    assert_eq!(*captured.borrow(), Some(SystemTheme::Dark));
}

#[test]
fn provide_system_theme_sets_current_theme() {
    let local = local_system_theme();
    let captured = Rc::new(RefCell::new(None));

    {
        let captured = Rc::clone(&captured);
        run_test_composition(move || {
            let captured = Rc::clone(&captured);
            let local = local.clone();
            ProvideSystemTheme(SystemTheme::Dark, move || {
                *captured.borrow_mut() = Some(local.current());
            });
        });
    }

    assert_eq!(*captured.borrow(), Some(SystemTheme::Dark));
}

#[test]
fn is_system_in_dark_theme_reads_current_theme() {
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

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "android"),
    not(target_os = "ios"),
    feature = "system-theme"
))]
#[test]
fn theme_from_text_reads_common_native_values() {
    assert_eq!(theme_from_text("'prefer-dark'"), Some(SystemTheme::Dark));
    assert_eq!(theme_from_text("Breeze Light"), Some(SystemTheme::Light));
    assert_eq!(theme_from_text("Adwaita"), None);
}
