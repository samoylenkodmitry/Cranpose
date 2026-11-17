//! Cranpose composables for headsets.
//!
//! An app writes a [`Subspace`] of [`SpatialPanel`]s with the same composables as on every
//! other platform. In a headset each panel is a layer of the headset's compositor, which keeps
//! its text sharp. The names follow Jetpack XR Compose. `docs/xr_architecture.md` in the
//! repository describes the design.
//!
//! The `openxr` feature adds the backend for Android headsets such as the Meta Quest. Its
//! [`android_main!`] starts the app as an immersive app. Elsewhere the composables draw flat.

#[cfg(any(test, all(feature = "openxr", target_os = "android")))]
mod pointer;
mod pose;
mod ray;
mod space;
mod spatial_panel;
mod subspace;
mod subspace_modifier;

mod android_main;
#[cfg(all(feature = "openxr", target_os = "android"))]
mod openxr;

#[doc(hidden)]
pub use cranpose as __cranpose;
#[cfg(all(feature = "openxr", target_os = "android"))]
#[doc(hidden)]
pub use openxr::run;
pub use pose::Pose;
pub use ray::{PanelHit, Ray};
pub use spatial_panel::SpatialPanel;
pub use subspace::Subspace;
pub use subspace_modifier::{DEFAULT_PANEL_SIZE, METERS_PER_DP, SubspaceModifier};
