//! The streamed scenes, which only an Android headset shows for now.

#[cfg(target_os = "android")]
mod android;
#[cfg(not(target_os = "android"))]
mod desktop;

#[cfg(target_os = "android")]
pub(crate) use android::StreamLayers;
#[cfg(not(target_os = "android"))]
pub(crate) use desktop::StreamLayers;
