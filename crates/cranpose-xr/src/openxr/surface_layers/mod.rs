//! The surfaces of the surface panels, which only an Android headset's compositor shows.

#[cfg(target_os = "android")]
mod android;
#[cfg(not(target_os = "android"))]
mod desktop;

#[cfg(target_os = "android")]
pub(crate) use android::SurfaceLayers;
#[cfg(not(target_os = "android"))]
pub(crate) use desktop::SurfaceLayers;
