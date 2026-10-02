use std::cell::OnceCell;

use cranpose_core::CompositionLocal;

#[derive(Default)]
pub(crate) struct ServiceLocals {
    pub(crate) accessibility_options:
        OnceCell<CompositionLocal<crate::accessibility_options::AccessibilityOptions>>,
    pub(crate) accessibility_state:
        OnceCell<CompositionLocal<crate::accessibility_state::AccessibilityState>>,
    pub(crate) audio: OnceCell<CompositionLocal<crate::audio::AudioPlayerRef>>,
    pub(crate) file_picker: OnceCell<CompositionLocal<crate::file_picker::FilePickerRef>>,
    pub(crate) haptics: OnceCell<CompositionLocal<crate::haptics::HapticsRef>>,
    pub(crate) http: OnceCell<CompositionLocal<crate::http::HttpClientRef>>,
    pub(crate) image_picker: OnceCell<CompositionLocal<crate::image_picker::ImagePickerRef>>,
    pub(crate) launch_args: OnceCell<CompositionLocal<crate::launch_args::LaunchArgsRef>>,
    pub(crate) notifier: OnceCell<CompositionLocal<crate::notifier::NotifierRef>>,
    pub(crate) share_sheet: OnceCell<CompositionLocal<crate::share_sheet::ShareSheetRef>>,
    pub(crate) theme: OnceCell<CompositionLocal<crate::theme::SystemTheme>>,
    pub(crate) uri_handler: OnceCell<CompositionLocal<crate::uri_handler::UriHandlerRef>>,
    pub(crate) lifecycle: OnceCell<CompositionLocal<crate::host::LifecycleState>>,
}

thread_local! {
    static LOCALS: ServiceLocals = ServiceLocals::default();
}

pub(crate) fn cached_local<T: Clone + 'static>(
    cell: impl FnOnce(&ServiceLocals) -> &OnceCell<CompositionLocal<T>>,
    initialize: impl FnOnce() -> CompositionLocal<T>,
) -> CompositionLocal<T> {
    LOCALS.with(|locals| cell(locals).get_or_init(initialize).clone())
}

#[cfg(test)]
#[path = "tests/composition_locals_tests.rs"]
mod tests;
