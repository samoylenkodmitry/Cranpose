use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_core::{CompositionLocal, OwnedMutableState, ProvidedValue, RuntimeHandle};
use cranpose_services::{SystemTheme, set_platform_system_theme};
use cranpose_ui::{EdgeInsets, composable, local_ime_insets, local_safe_area_insets};

#[cfg(feature = "localization")]
#[path = "platform_languages.rs"]
mod languages;

#[derive(Default)]
pub(crate) struct PlatformEnvironment {
    #[cfg(feature = "localization")]
    languages: Rc<languages::SystemLanguagesState>,
    #[cfg(feature = "webview")]
    pub(crate) native_views: crate::native_view::NativeViewHost,
    safe_area: PlatformInsets,
    ime_insets: PlatformInsets,
}

#[derive(Default)]
struct PlatformInsets {
    value: Cell<EdgeInsets>,
    state: RefCell<Option<(RuntimeHandle, OwnedMutableState<EdgeInsets>)>>,
}

impl PlatformInsets {
    fn set(&self, value: EdgeInsets) -> bool {
        if self.value.replace(value) == value {
            return false;
        }
        if let Some((_, state)) = self.state.borrow().as_ref() {
            state.set(value);
        }
        true
    }

    fn provide(
        &self,
        local: CompositionLocal<EdgeInsets>,
        runtime: &RuntimeHandle,
    ) -> ProvidedValue {
        let mut binding = self.state.borrow_mut();
        if binding
            .as_ref()
            .is_none_or(|(owner, _)| owner.id() != runtime.id())
        {
            *binding = Some((
                runtime.clone(),
                OwnedMutableState::with_runtime_structural_eq(self.value.get(), runtime.clone()),
            ));
        }
        let (_, state) = binding
            .as_ref()
            .expect("insets initialized for this runtime");
        local.provides_state(state.clone())
    }
}

impl PlatformEnvironment {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    #[cfg(all(feature = "localization", feature = "android", target_os = "android"))]
    pub(crate) fn refresh_languages(&self) -> bool {
        self.languages.refresh()
    }

    #[cfg_attr(not(any(target_os = "android", target_os = "ios")), expect(dead_code))]
    pub(crate) fn set_safe_area(&self, insets: EdgeInsets) -> bool {
        self.safe_area.set(insets)
    }

    #[cfg_attr(not(any(target_os = "android", target_os = "ios")), expect(dead_code))]
    pub(crate) fn set_ime_insets(&self, insets: EdgeInsets) -> bool {
        self.ime_insets.set(insets)
    }

    pub(crate) fn set_system_theme(&self, theme: SystemTheme) -> bool {
        let changed = cranpose_services::default_system_theme() != theme;
        set_platform_system_theme(theme);
        changed
    }

    pub(crate) fn compose_root(&self, content: impl FnOnce()) {
        let theme = cranpose_services::default_system_theme();
        let launch_args = cranpose_services::launch_args();
        let runtime = cranpose_core::with_current_composer(|composer| composer.runtime_handle());
        cranpose_core::CompositionLocalProvider(
            [
                self.safe_area.provide(local_safe_area_insets(), &runtime),
                self.ime_insets.provide(local_ime_insets(), &runtime),
                cranpose_services::local_system_theme().provides(theme),
                cranpose_services::local_launch_args().provides(launch_args),
            ],
            || {
                RootBackHandler();
                let content = || {
                    #[cfg(feature = "webview")]
                    self.native_views.provide(content);
                    #[cfg(not(feature = "webview"))]
                    content();
                };
                #[cfg(feature = "localization")]
                languages::ProvideSystemLanguages(self.languages.clone(), content);
                #[cfg(not(feature = "localization"))]
                content();
            },
        );
    }
}

/// The way out of the dialog or the menu on top, in a scope of its own. It
/// reads the popup registry's revision; read from the root scope, every popup
/// that opened would recompose the whole app, which registers the popup again
/// and bumps the revision again, with no end.
#[composable]
fn RootBackHandler() {
    let modal_open = cranpose_ui::modal_depth() > 0;
    let popup_open = cranpose_ui::dismissable_popup_open();
    crate::BackHandler(modal_open || popup_open, || {
        if !cranpose_ui::dispatch_modal_back() {
            cranpose_ui::dismiss_top_popup();
        }
    });
}
