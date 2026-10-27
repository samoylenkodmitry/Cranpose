use cranpose_core::{CompositionLocal, compositionLocalOf};

/// The kind of device the application runs on, as the type part of Android's
/// `Configuration.uiMode` (`UI_MODE_TYPE_TELEVISION` is
/// [`UiModeType::Television`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum UiModeType {
    /// A phone, a tablet, a desktop computer or a browser.
    #[default]
    Normal,
    /// A device in a desk dock.
    Desk,
    /// A device in a car dock or a car's own display.
    Car,
    /// A TV: Android TV, Google TV or Apple TV, driven by a remote.
    Television,
    /// A device with no display of its own, or one that is not mainly for
    /// people to look at.
    Appliance,
    /// A watch.
    Watch,
    /// A virtual reality headset.
    VrHeadset,
}

/// What the platform tells the application about the device, as
/// `LocalConfiguration.current` holds in Compose.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Configuration {
    /// The kind of device.
    pub ui_mode_type: UiModeType,
}

/// CompositionLocal carrying the [`Configuration`], as `LocalConfiguration`
/// does in Compose. A TV layout reads
/// `local_configuration().current().ui_mode_type == UiModeType::Television`.
pub fn local_configuration() -> CompositionLocal<Configuration> {
    crate::environment_locals::cached_local(
        |locals| &locals.configuration,
        || compositionLocalOf(Configuration::default),
    )
}
