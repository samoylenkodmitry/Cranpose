use std::{cell::Cell, rc::Rc};

use cranpose_services::{SystemTheme, set_platform_system_theme};
use cranpose_ui::{EdgeInsets, local_ime_insets, local_safe_area_insets};

#[derive(Default)]
pub(crate) struct PlatformEnvironment {
    safe_area: Cell<EdgeInsets>,
    ime_insets: Cell<EdgeInsets>,
}

impl PlatformEnvironment {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    #[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
    pub(crate) fn set_safe_area(&self, insets: EdgeInsets) -> bool {
        let changed = self.safe_area.get() != insets;
        self.safe_area.set(insets);
        changed
    }

    #[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
    pub(crate) fn set_ime_insets(&self, insets: EdgeInsets) -> bool {
        let changed = self.ime_insets.get() != insets;
        self.ime_insets.set(insets);
        changed
    }

    pub(crate) fn set_system_theme(&self, theme: SystemTheme) -> bool {
        let changed = cranpose_services::default_system_theme() != theme;
        set_platform_system_theme(theme);
        changed
    }

    pub(crate) fn compose_root(&self, content: impl FnOnce()) {
        let theme = cranpose_services::default_system_theme();
        let launch_args = cranpose_services::launch_args();
        cranpose_core::CompositionLocalProvider(
            vec![
                local_safe_area_insets().provides(self.safe_area.get()),
                local_ime_insets().provides(self.ime_insets.get()),
                cranpose_services::local_system_theme().provides(theme),
                cranpose_services::local_launch_args().provides(launch_args),
            ],
            || {
                crate::BackHandler(cranpose_ui::modal_depth() > 0, || {
                    cranpose_ui::dispatch_modal_back();
                });
                content();
            },
        );
    }
}
