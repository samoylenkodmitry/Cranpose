use std::{cell::OnceCell, rc::Rc};

use cranpose_core::{CompositionLocal, compositionLocalOf};
use cranpose_ui::Locale;

/// Reads ordered host language preferences, preserving regions and script subtags.
/// Android includes the application's locale override from its resources configuration.
pub fn system_languages() -> Vec<Locale> {
    let supplied = cranpose_services::host_controller()
        .map_or_else(Vec::new, |host| host.preferred_languages());
    let tags = if supplied.is_empty() {
        sys_locale::get_locales().collect()
    } else {
        supplied
    };
    tags.into_iter()
        .filter_map(|tag| Locale::parse(&tag).ok())
        .collect()
}

/// The current host language preferences for this application subtree.
/// Native configuration changes and app resumes refresh this value automatically.
pub fn local_system_languages() -> CompositionLocal<Rc<[Locale]>> {
    thread_local! {
        static LOCAL: OnceCell<CompositionLocal<Rc<[Locale]>>> = const { OnceCell::new() };
    }
    LOCAL.with(|local| {
        local
            .get_or_init(|| compositionLocalOf(|| system_languages().into()))
            .clone()
    })
}
