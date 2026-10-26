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
#[cfg(any(
    all(
        feature = "desktop-shell",
        feature = "renderer-wgpu",
        target_os = "macos"
    ),
    all(feature = "ios", target_os = "ios")
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
