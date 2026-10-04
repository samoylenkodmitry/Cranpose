use std::cell::OnceCell;

use cranpose_core::{CompositionLocal, OwnedMutableState, StaticCompositionLocal};

pub(crate) struct EnvironmentLocals {
    pub(crate) density: OnceCell<CompositionLocal<crate::Density>>,
    pub(crate) direction: OnceCell<CompositionLocal<crate::LayoutDirection>>,
    pub(crate) safe_area: OnceCell<CompositionLocal<crate::EdgeInsets>>,
    pub(crate) ime: OnceCell<CompositionLocal<crate::EdgeInsets>>,
    pub(crate) clipboard: OnceCell<CompositionLocal<crate::clipboard_session::ClipboardManager>>,
    pub(crate) focus_manager: OnceCell<CompositionLocal<crate::FocusManager>>,
    pub(crate) announcer: OnceCell<CompositionLocal<crate::Announcer>>,
    pub(crate) modal_depth: OnceCell<CompositionLocal<usize>>,
    pub(crate) lazy_item_key: OnceCell<CompositionLocal<Option<u64>>>,
    pub(crate) bring_into_view: OnceCell<CompositionLocal<Option<crate::BringIntoViewResponder>>>,
    pub(crate) selection_registrar:
        OnceCell<CompositionLocal<Option<crate::selection_container::SelectionRegistrar>>>,
    pub(crate) on_light_surface: OnceCell<CompositionLocal<bool>>,
    pub(crate) popup_registry: OnceCell<StaticCompositionLocal<crate::widgets::PopupRegistry>>,
    pub(crate) popup_viewport:
        OnceCell<StaticCompositionLocal<OwnedMutableState<cranpose_ui_graphics::Size>>>,
    #[cfg(feature = "localization")]
    pub(crate) translator: OnceCell<CompositionLocal<Option<crate::localization::Translator>>>,
    #[cfg(feature = "localization")]
    pub(crate) text_locales: OnceCell<CompositionLocal<Option<crate::text::LocaleList>>>,
    #[cfg(feature = "localization")]
    pub(crate) localization_controller:
        OnceCell<CompositionLocal<Option<crate::LocalizationController>>>,
}

thread_local! {
    pub(crate) static ENVIRONMENT_LOCALS: EnvironmentLocals = const { EnvironmentLocals {
        density: OnceCell::new(),
        direction: OnceCell::new(),
        safe_area: OnceCell::new(),
        ime: OnceCell::new(),
        clipboard: OnceCell::new(),
        focus_manager: OnceCell::new(),
        announcer: OnceCell::new(),
        modal_depth: OnceCell::new(),
        lazy_item_key: OnceCell::new(),
        bring_into_view: OnceCell::new(),
        selection_registrar: OnceCell::new(),
        on_light_surface: OnceCell::new(),
        popup_registry: OnceCell::new(),
        popup_viewport: OnceCell::new(),
        #[cfg(feature = "localization")]
        translator: OnceCell::new(),
        #[cfg(feature = "localization")]
        text_locales: OnceCell::new(),
        #[cfg(feature = "localization")]
        localization_controller: OnceCell::new(),
    } };
}

pub(crate) fn cached_local<T: Clone>(
    cell: impl FnOnce(&EnvironmentLocals) -> &OnceCell<T>,
    initialize: impl FnOnce() -> T,
) -> T {
    ENVIRONMENT_LOCALS.with(|locals| cell(locals).get_or_init(initialize).clone())
}
