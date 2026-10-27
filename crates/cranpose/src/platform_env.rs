use std::{cell::Cell, rc::Rc};

use cranpose_services::{SystemTheme, set_platform_system_theme};
use cranpose_ui::{
    Configuration, EdgeInsets, UiModeType, composable, local_configuration, local_ime_insets,
    local_safe_area_insets,
};

const TV_OVERSCAN_SIDE: f32 = 48.0;
const TV_OVERSCAN_EDGE: f32 = 27.0;

#[cfg(feature = "localization")]
#[path = "platform_languages.rs"]
mod languages;

#[derive(Default)]
pub(crate) struct PlatformEnvironment {
    #[cfg(feature = "localization")]
    languages: Rc<languages::SystemLanguagesState>,
    #[cfg(feature = "webview")]
    pub(crate) native_views: crate::native_view::NativeViewHost,
    safe_area: Cell<EdgeInsets>,
    ime_insets: Cell<EdgeInsets>,
    configuration: Cell<Configuration>,
}

impl PlatformEnvironment {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    #[cfg(all(feature = "localization", feature = "android", target_os = "android"))]
    pub(crate) fn refresh_languages(&self) -> bool {
        self.languages.refresh()
    }

    // The window's edges the system draws on. A TV keeps content inside the
    // overscan margins the Android TV design guide names, 48 dp at the sides
    // and 27 dp at the top and bottom, as tvOS reports in its safe area.
    pub(crate) fn safe_area(&self) -> EdgeInsets {
        let insets = self.safe_area.get();
        if self.configuration.get().ui_mode_type != UiModeType::Television {
            return insets;
        }
        EdgeInsets {
            left: insets.left.max(TV_OVERSCAN_SIDE),
            top: insets.top.max(TV_OVERSCAN_EDGE),
            right: insets.right.max(TV_OVERSCAN_SIDE),
            bottom: insets.bottom.max(TV_OVERSCAN_EDGE),
        }
    }

    #[cfg_attr(
        not(any(
            target_os = "android",
            target_os = "ios",
            feature = "framebuffer",
            test
        )),
        expect(
            dead_code,
            reason = "only Android, iOS and the framebuffer backend report a safe area"
        )
    )]
    pub(crate) fn set_safe_area(&self, insets: EdgeInsets) -> bool {
        let changed = self.safe_area.get() != insets;
        self.safe_area.set(insets);
        changed
    }

    #[cfg_attr(
        not(any(target_os = "android", target_os = "ios")),
        expect(dead_code, reason = "only Android and iOS report keyboard insets")
    )]
    pub(crate) fn set_ime_insets(&self, insets: EdgeInsets) -> bool {
        let changed = self.ime_insets.get() != insets;
        self.ime_insets.set(insets);
        changed
    }

    #[cfg_attr(
        not(any(target_os = "android", feature = "watchos", test)),
        expect(dead_code, reason = "only Android and watchOS report a device kind")
    )]
    pub(crate) fn set_configuration(&self, configuration: Configuration) -> bool {
        self.configuration.replace(configuration) != configuration
    }

    pub(crate) fn set_system_theme(&self, theme: SystemTheme) -> bool {
        // Compared with the theme already known: detecting one here would
        // run the detection a theme reported at launch exists to skip.
        let changed = cranpose_services::known_system_theme() != Some(theme);
        set_platform_system_theme(theme);
        changed
    }

    pub(crate) fn compose_root(&self, content: impl FnOnce()) {
        let theme = cranpose_services::default_system_theme();
        let launch_args = cranpose_services::launch_args();
        cranpose_core::CompositionLocalProvider(
            [
                local_safe_area_insets().provides(self.safe_area()),
                local_ime_insets().provides(self.ime_insets.get()),
                local_configuration().provides(self.configuration.get()),
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
                languages::ProvideSystemLanguages(Rc::clone(&self.languages), content);
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

#[cfg(test)]
#[path = "tests/platform_env_tests.rs"]
mod tests;
