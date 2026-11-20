//! Hardware that an application cannot run without.

/// Hardware an application cannot run without.
///
/// Every other feature stays optional, so the application reaches devices
/// that lack the hardware and asks the framework at run time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Demand {
    /// A camera.
    Camera,
    /// A microphone.
    Microphone,
    /// A watch.
    Watch,
    /// Telephony.
    Telephony,
    /// Bluetooth.
    Bluetooth,
    /// Near-field communication.
    Nfc,
    /// Wi-Fi.
    Wifi,
    /// Location hardware.
    Location,
}

impl Demand {
    /// The Android feature name this demand becomes.
    pub const fn android_feature(self) -> &'static str {
        match self {
            Self::Camera => "android.hardware.camera",
            Self::Microphone => "android.hardware.microphone",
            Self::Watch => "android.hardware.type.watch",
            Self::Telephony => "android.hardware.telephony",
            Self::Bluetooth => "android.hardware.bluetooth",
            Self::Nfc => "android.hardware.nfc",
            Self::Wifi => "android.hardware.wifi",
            Self::Location => "android.hardware.location",
        }
    }

    /// The name this demand carries in the build's own files.
    pub const fn name(self) -> &'static str {
        self.android_feature()
    }
}
