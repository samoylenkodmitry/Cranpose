use cranpose_core::{compositionLocalOf, CompositionLocal, CompositionLocalProvider};
use cranpose_macros::composable;
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemTheme {
    Light,
    Dark,
}

pub fn default_system_theme() -> SystemTheme {
    #[cfg(all(
        not(target_arch = "wasm32"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
    {
        match dark_light::detect() {
            Ok(dark_light::Mode::Dark) => SystemTheme::Dark,
            _ => SystemTheme::Light,
        }
    }

    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|window| {
                window
                    .match_media("(prefers-color-scheme: dark)")
                    .ok()
                    .flatten()
            })
            .map(|query| {
                if query.matches() {
                    SystemTheme::Dark
                } else {
                    SystemTheme::Light
                }
            })
            .unwrap_or(SystemTheme::Light)
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        SystemTheme::Light
    }
}

pub fn local_system_theme() -> CompositionLocal<SystemTheme> {
    thread_local! {
        static LOCAL_SYSTEM_THEME: RefCell<Option<CompositionLocal<SystemTheme>>> = const { RefCell::new(None) };
    }

    LOCAL_SYSTEM_THEME.with(|cell| {
        let mut local = cell.borrow_mut();
        if local.is_none() {
            *local = Some(compositionLocalOf(default_system_theme));
        }
        local
            .as_ref()
            .expect("System theme composition local must be initialized")
            .clone()
    })
}

#[allow(non_snake_case)]
#[composable]
pub fn ProvideSystemTheme(theme: SystemTheme, content: impl FnOnce()) {
    let local = local_system_theme();
    CompositionLocalProvider(vec![local.provides(theme)], move || {
        content();
    });
}

#[allow(non_snake_case)]
#[composable]
pub fn isSystemInDarkTheme() -> bool {
    matches!(local_system_theme().current(), SystemTheme::Dark)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_test_composition;
    use cranpose_core::CompositionLocalProvider;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn default_system_theme_returns_supported_variant() {
        assert!(matches!(
            default_system_theme(),
            SystemTheme::Light | SystemTheme::Dark
        ));
    }

    #[test]
    fn local_system_theme_can_be_overridden() {
        let local = local_system_theme();
        let captured = Rc::new(RefCell::new(None));

        {
            let captured = Rc::clone(&captured);
            let local_for_provider = local.clone();
            let local_for_read = local.clone();
            run_test_composition(move || {
                let captured = Rc::clone(&captured);
                let local_for_read = local_for_read.clone();
                CompositionLocalProvider(
                    vec![local_for_provider.provides(SystemTheme::Dark)],
                    move || {
                        *captured.borrow_mut() = Some(local_for_read.current());
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
            let local = local.clone();
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
}
