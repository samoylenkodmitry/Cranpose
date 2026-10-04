use std::cell::OnceCell;

use cranpose_core::CompositionLocal;

pub(crate) struct EnvironmentLocals {
    pub(crate) density: OnceCell<CompositionLocal<crate::Density>>,
    pub(crate) direction: OnceCell<CompositionLocal<crate::LayoutDirection>>,
    pub(crate) safe_area: OnceCell<CompositionLocal<crate::EdgeInsets>>,
    pub(crate) ime: OnceCell<CompositionLocal<crate::EdgeInsets>>,
    #[cfg(feature = "localization")]
    pub(crate) translator: OnceCell<CompositionLocal<Option<crate::localization::Translator>>>,
    #[cfg(feature = "localization")]
    pub(crate) text_locales: OnceCell<CompositionLocal<Option<crate::text::LocaleList>>>,
}

thread_local! {
    pub(crate) static ENVIRONMENT_LOCALS: EnvironmentLocals = const { EnvironmentLocals {
        density: OnceCell::new(),
        direction: OnceCell::new(),
        safe_area: OnceCell::new(),
        ime: OnceCell::new(),
        #[cfg(feature = "localization")]
        translator: OnceCell::new(),
        #[cfg(feature = "localization")]
        text_locales: OnceCell::new(),
    } };
}
