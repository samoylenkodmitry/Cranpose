//! Cranpose composables for headsets.
//!
//! An app writes a [`Subspace`] of [`SpatialPanel`]s with the same composables as on every
//! other platform. In a headset each panel is a layer of the headset's compositor, which keeps
//! its text sharp. The names follow Jetpack XR Compose. `docs/xr_architecture.md` in the
//! repository describes the design.
//!
//! A [`SpatialExternalSurface`] gives the app a surface that a hardware video decoder or a camera
//! writes into. The compositor shows its pictures as a layer with no copy.
//!
//! The `openxr` feature adds the backend for headsets. On Android, as on a Meta Quest, its
//! [`android_main!`] starts the app as an immersive app. On Linux and Windows its `run` starts
//! the app in the headset of the machine's OpenXR runtime (SteamVR, Monado, Meta Link) and opens
//! a flat window when the machine has none. On macOS its `run` opens the flat window. Elsewhere
//! the composables draw flat.

#[cfg(any(test, headset))]
mod aim_filter;
#[cfg(any(test, headset))]
mod beam_pose;
#[cfg(any(test, headset))]
mod carry;
mod locals;
#[cfg(any(test, headset))]
mod pointer;
mod pose;
mod ray;
mod space;
mod spatial_capabilities;
mod spatial_curved_row;
mod spatial_environment;
mod spatial_external_surface;
mod spatial_panel;
mod stereo_mode;
mod subspace;
mod subspace_modifier;

mod android_main;
#[cfg(all(desktop_run, not(headset)))]
mod flat_run;
#[cfg(headset)]
mod openxr;
#[cfg(desktop_run)]
mod run_error;

#[doc(hidden)]
pub use cranpose as __cranpose;
pub use cranpose::ExternalSurface;
#[cfg(all(desktop_run, not(headset)))]
pub use flat_run::run;
#[cfg(all(headset, target_os = "android"))]
#[doc(hidden)]
pub use openxr::run;
#[cfg(all(headset, not(target_os = "android")))]
pub use openxr::run;
pub use pose::Pose;
pub use ray::{PanelHit, Ray};
#[cfg(desktop_run)]
pub use run_error::RunError;
pub use spatial_capabilities::{SpatialCapabilities, local_spatial_capabilities};
pub use spatial_curved_row::SpatialCurvedRow;
pub use spatial_environment::{SpatialEnvironment, local_spatial_environment};
pub use spatial_external_surface::{SpatialExternalSurface, SpatialExternalSurfaceScope};
pub use spatial_panel::SpatialPanel;
pub use stereo_mode::StereoMode;
pub use subspace::Subspace;
pub use subspace_modifier::{DEFAULT_PANEL_SIZE, METERS_PER_DP, SubspaceModifier};
