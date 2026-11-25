#[cfg(any(
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "watchos", target_os = "watchos")
))]
pub(crate) mod audio_session;
#[cfg(any(
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(
        feature = "camera-desktop",
        feature = "desktop-shell",
        feature = "renderer-wgpu",
        target_os = "macos"
    )
))]
pub(crate) mod camera;
#[cfg(all(
    feature = "device-auth",
    feature = "ios",
    feature = "renderer-wgpu",
    target_os = "ios"
))]
pub(crate) mod device_auth;
#[cfg(any(
    test,
    all(
        feature = "device-auth",
        feature = "ios",
        feature = "renderer-wgpu",
        target_os = "ios"
    )
))]
pub(crate) mod device_auth_codes;
#[cfg(all(
    feature = "device-motion",
    any(
        all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
        all(feature = "watchos", target_os = "watchos")
    )
))]
pub(crate) mod device_motion;
#[cfg(any(
    all(
        feature = "media-backend",
        feature = "desktop-shell",
        feature = "renderer-wgpu",
        target_os = "macos"
    ),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios")
))]
pub(crate) mod media_source;
#[cfg(any(
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(feature = "watchos", target_os = "watchos")
))]
pub(crate) mod microphone;
#[cfg(any(
    all(feature = "ios", target_os = "ios"),
    all(feature = "watchos", target_os = "watchos")
))]
pub(crate) mod mobile;
#[cfg(any(
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(
        feature = "desktop-shell",
        feature = "renderer-wgpu",
        target_os = "macos"
    )
))]
pub(crate) mod network;
#[cfg(any(
    all(
        feature = "media-backend",
        feature = "desktop-shell",
        feature = "renderer-wgpu",
        target_os = "macos"
    ),
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios")
))]
pub(crate) mod now_playing;
#[cfg(all(
    feature = "video",
    feature = "renderer-wgpu",
    any(target_os = "macos", target_os = "ios")
))]
pub(crate) mod nv12_converter;
#[cfg(all(feature = "video", any(target_os = "macos", target_os = "ios")))]
pub(crate) mod player;
#[cfg(all(feature = "video", any(target_os = "macos", target_os = "ios")))]
mod player_media;
#[cfg(all(feature = "video", feature = "ios", target_os = "ios"))]
mod player_route;
#[cfg(any(
    all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
    all(
        feature = "desktop-shell",
        feature = "renderer-wgpu",
        target_os = "macos"
    )
))]
pub(crate) mod thermal;
#[cfg(all(
    feature = "wearable",
    any(
        all(feature = "ios", feature = "renderer-wgpu", target_os = "ios"),
        all(feature = "watchos", target_os = "watchos")
    )
))]
pub(crate) mod wearable;
