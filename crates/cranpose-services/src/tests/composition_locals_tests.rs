use std::{cell::RefCell, rc::Rc};

use cranpose_core::CompositionLocalProvider;

use crate::{SystemTheme, local_accessibility_state, local_system_theme};

fn initialize_service_locals() {
    crate::local_accessibility_options();
    crate::local_audio();
    crate::file_picker::local_file_picker();
    crate::local_haptics();
    crate::host::local_lifecycle_state();
    crate::http::local_http_client();
    crate::local_image_picker();
    crate::local_launch_args();
    crate::local_notifier();
    crate::local_share_sheet();
    crate::local_uri_handler();
}

#[test]
fn service_locals_preserve_nested_providers_after_other_services_initialize() {
    let observed = Rc::new(RefCell::new(Vec::new()));
    let output = Rc::clone(&observed);
    crate::run_test_composition(move || {
        let theme = local_system_theme();
        let accessibility = local_accessibility_state();
        CompositionLocalProvider([theme.provides(SystemTheme::Dark)], || {
            initialize_service_locals();
            output.borrow_mut().push(local_system_theme().current());
            CompositionLocalProvider([local_system_theme().provides(SystemTheme::Light)], || {
                output.borrow_mut().push(theme.current());
            });
            output.borrow_mut().push(local_system_theme().current());
            assert_eq!(
                accessibility.current(),
                crate::platform_accessibility_state()
            );
        });
    });
    assert_eq!(
        *observed.borrow(),
        [SystemTheme::Dark, SystemTheme::Light, SystemTheme::Dark]
    );
}

#[test]
fn service_locals_keep_platform_defaults_isolated_between_threads() {
    let workers = [SystemTheme::Light, SystemTheme::Dark].map(|theme| {
        std::thread::spawn(move || {
            initialize_service_locals();
            crate::set_platform_system_theme(theme);
            let output = Rc::new(RefCell::new(None));
            let observed = Rc::clone(&output);
            crate::run_test_composition(move || {
                *observed.borrow_mut() = Some(local_system_theme().current());
            });
            let result = *output.borrow();
            crate::clear_platform_system_theme();
            assert_eq!(result, Some(theme));
        })
    });
    for worker in workers {
        worker.join().expect("service-local worker succeeds");
    }
}
