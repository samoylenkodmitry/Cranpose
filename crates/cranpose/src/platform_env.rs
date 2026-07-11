//! Platform → composition environment plumbing shared by every shell.
//!
//! The platform drivers (desktop, android, ios, web) publish live environment
//! values here — safe-area insets, soft-keyboard insets, the OS light/dark
//! theme — and wrap the app's root content in one provider that re-reads them
//! on every root render. A plain shared cell is not reactive on its own, so a
//! driver that changes a value must force a root render (for example
//! `AppShell::request_root_render`) for composition to observe it.

use cranpose_services::{set_platform_system_theme, SystemTheme};
use cranpose_ui::{local_ime_insets, local_safe_area_insets, EdgeInsets};
use std::cell::Cell;
use std::rc::Rc;

/// Live environment values a platform driver feeds into composition.
#[derive(Default)]
pub(crate) struct PlatformEnvironment {
    safe_area: Cell<EdgeInsets>,
    ime_insets: Cell<EdgeInsets>,
}

impl PlatformEnvironment {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// Update the safe-area insets. Returns true when the value changed and a
    /// root render is required. (Only mobile drivers publish non-zero insets.)
    #[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
    pub(crate) fn set_safe_area(&self, insets: EdgeInsets) -> bool {
        let changed = self.safe_area.get() != insets;
        self.safe_area.set(insets);
        changed
    }

    /// Update the soft-keyboard insets. Returns true when the value changed
    /// and a root render is required. (Only mobile drivers publish non-zero
    /// insets.)
    #[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
    pub(crate) fn set_ime_insets(&self, insets: EdgeInsets) -> bool {
        let changed = self.ime_insets.get() != insets;
        self.ime_insets.set(insets);
        changed
    }

    /// Record the OS-reported theme. Returns true when it changed and a root
    /// render is required. The value flows through
    /// [`cranpose_services::default_system_theme`], so both the composition
    /// local and plain accessor observe it.
    pub(crate) fn set_system_theme(&self, theme: SystemTheme) -> bool {
        let changed = cranpose_services::default_system_theme() != theme;
        set_platform_system_theme(theme);
        changed
    }

    /// Wrap the app's root content with the environment providers. Platform
    /// drivers call this inside the shell's root closure.
    pub(crate) fn compose_root(&self, content: impl FnOnce()) {
        let theme = cranpose_services::default_system_theme();
        cranpose_core::CompositionLocalProvider(
            vec![
                local_safe_area_insets().provides(self.safe_area.get()),
                local_ime_insets().provides(self.ime_insets.get()),
                cranpose_services::local_system_theme().provides(theme),
            ],
            content,
        );
    }
}
