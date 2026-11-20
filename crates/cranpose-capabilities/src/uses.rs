//! The constructors of each service an application can declare.

use crate::{Service, Use};

impl Use {
    /// The camera, with what the person is told before it opens.
    pub const fn camera(reason: &'static str) -> Self {
        Self {
            service: Service::Camera,
            reason: Some(reason),
        }
    }

    /// Reading the photo library, with what the person is told.
    pub const fn photo_library(reason: &'static str) -> Self {
        Self {
            service: Service::PhotoLibrary,
            reason: Some(reason),
        }
    }

    /// Writing to the photo library, with what the person is told.
    pub const fn photo_library_add(reason: &'static str) -> Self {
        Self {
            service: Service::PhotoLibraryAdd,
            reason: Some(reason),
        }
    }

    /// The microphone, with what the person is told.
    pub const fn microphone(reason: &'static str) -> Self {
        Self {
            service: Service::Microphone,
            reason: Some(reason),
        }
    }

    /// Where the device is, with what the person is told.
    pub const fn location(reason: &'static str) -> Self {
        Self {
            service: Service::Location,
            reason: Some(reason),
        }
    }

    /// Notifications the person sees.
    pub const fn notifications() -> Self {
        Self {
            service: Service::Notifications,
            reason: None,
        }
    }

    /// Work that carries on with the application off screen.
    pub const fn background() -> Self {
        Self {
            service: Service::Background,
            reason: None,
        }
    }

    /// Media playback that carries on with the application off screen.
    pub const fn media() -> Self {
        Self {
            service: Service::Media,
            reason: None,
        }
    }

    /// Purchases through the platform store.
    pub const fn billing() -> Self {
        Self {
            service: Service::Billing,
            reason: None,
        }
    }

    /// A window drawn above other applications.
    pub const fn overlay() -> Self {
        Self {
            service: Service::Overlay,
            reason: None,
        }
    }

    /// The vibrator.
    pub const fn haptics() -> Self {
        Self {
            service: Service::Haptics,
            reason: None,
        }
    }

    /// Recording that can start from the background; see
    /// `cranpose_services::hold_microphone_standby`.
    pub const fn microphone_standby() -> Self {
        Self {
            service: Service::MicrophoneStandby,
            reason: None,
        }
    }

    /// Messages and streams to the same application on the paired phone or
    /// watch; see `cranpose_services::wearable`.
    pub const fn wearable() -> Self {
        Self {
            service: Service::Wearable,
            reason: None,
        }
    }

    /// Handing a downloaded package to the system installer.
    pub const fn update() -> Self {
        Self {
            service: Service::Update,
            reason: None,
        }
    }

    /// The network, and whether the device is on one.
    pub const fn network() -> Self {
        Self {
            service: Service::Network,
            reason: None,
        }
    }

    /// The wearer's heart rate, with what the person is told before the
    /// sensor is read. Declaring it is the only way an application's build
    /// asks for the permission; reading it still takes an explicit request
    /// at run time.
    pub const fn heart_rate(reason: &'static str) -> Self {
        Self {
            service: Service::HeartRate,
            reason: Some(reason),
        }
    }

    /// Asking the owner of the device to unlock, with what Face ID tells
    /// the person the first time; see
    /// `cranpose_services::authenticate_device_owner`.
    pub const fn device_auth(reason: &'static str) -> Self {
        Self {
            service: Service::DeviceAuth,
            reason: Some(reason),
        }
    }

    /// The rotation of the device, from its motion sensors; see
    /// `cranpose_services::device_motion`. Reading it asks the person for
    /// nothing on Android and iOS.
    pub const fn device_motion() -> Self {
        Self {
            service: Service::DeviceMotion,
            reason: None,
        }
    }

    /// The joints of the user's hands, which a headset's cameras track; see
    /// `cranpose_xr::XrFrame`. Declaring it is the only way an application's
    /// build asks for the permission and for the runtime's extension.
    pub const fn hand_tracking() -> Self {
        Self {
            service: Service::HandTracking,
            reason: None,
        }
    }

    /// The joints of the user's body, which a headset estimates; see
    /// `cranpose_xr::XrFrame`.
    pub const fn body_tracking() -> Self {
        Self {
            service: Service::BodyTracking,
            reason: None,
        }
    }

    /// The user's facial expression, which a headset estimates from its
    /// cameras or from the microphone; see `cranpose_xr::XrFrame`.
    pub const fn face_tracking() -> Self {
        Self {
            service: Service::FaceTracking,
            reason: None,
        }
    }

    /// Where the user's eyes look; see `cranpose_xr::XrFrame`.
    pub const fn eye_tracking() -> Self {
        Self {
            service: Service::EyeTracking,
            reason: None,
        }
    }

    /// Places in the room that stay put, and that the application keeps for its next start; see
    /// `cranpose_xr::Anchors`. Jetpack XR's `Config.anchorPersistence`, which also covers the
    /// creation of anchors. Declaring it is the only way an application's build asks for the
    /// permission and for the runtime's extensions.
    pub const fn anchors() -> Self {
        Self {
            service: Service::Anchors,
            reason: None,
        }
    }

    /// The planes of the room, such as its walls, floor and tables; see
    /// `cranpose_xr::Planes`. Jetpack XR's `Config.planeTracking`. The user allows it with a
    /// prompt.
    pub const fn plane_tracking() -> Self {
        Self {
            service: Service::PlaneTracking,
            reason: None,
        }
    }

    /// The headset's own 3D models of its controllers; see `cranpose_xr::ControllerModels`. A Meta
    /// Quest hands them to an application that asks for them in its manifest. `cranpose-xr` then
    /// draws them at the controllers' grips in place of its plain built-in model. An application
    /// that declares nothing still shows the built-in model. Asking costs nothing: the system
    /// grants the permission at install.
    pub const fn controller_models() -> Self {
        Self {
            service: Service::ControllerModels,
            reason: None,
        }
    }

    /// Which service this is.
    pub const fn service(self) -> Service {
        self.service
    }

    /// The sentence the person is shown, for the services that have one.
    pub const fn reason(self) -> Option<&'static str> {
        self.reason
    }
}

#[cfg(test)]
#[path = "tests/uses_tests.rs"]
mod tests;
