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
//! A [`SpatialFrameEffect`] gives an app the headset's state in each frame: the head, the eyes,
//! the controllers and the recenter event, as an [`XrFrame`]. [`Haptics`] plays vibrations on the
//! controllers. A pure Cranpose app can stream a headset to a host with them.
//!
//! The `openxr` feature adds the backend for Android headsets such as the Meta Quest. Its
//! [`android_main!`] starts the app as an immersive app. Elsewhere the composables draw flat.

#[cfg(any(test, all(feature = "openxr", target_os = "android")))]
mod aim_filter;
#[cfg(any(test, all(feature = "openxr", target_os = "android")))]
mod beam_pose;
#[cfg(any(test, all(feature = "openxr", target_os = "android")))]
mod carry;
mod controller;
#[cfg(any(test, all(feature = "openxr", target_os = "android")))]
mod controller_bindings;
mod field_of_view;
mod frame_effects;
mod hand;
mod haptics;
mod headset_properties;
mod interaction_profile;
mod locals;
#[cfg(any(test, all(feature = "openxr", target_os = "android")))]
mod pointer;
mod pose;
mod ray;
mod recenter;
mod render_viewpoint;
mod space;
mod spatial_capabilities;
mod spatial_curved_row;
mod spatial_environment;
mod spatial_external_surface;
mod spatial_frame_effect;
mod spatial_panel;
mod stereo_mode;
mod subspace;
mod subspace_modifier;
mod xr_frame;

mod android_main;
#[cfg(all(feature = "openxr", target_os = "android"))]
mod openxr;

pub use controller::{Button, Controller, Thumbstick, TrackedPose, Trigger};
#[doc(hidden)]
pub use cranpose as __cranpose;
pub use cranpose::ExternalSurface;
pub use field_of_view::FieldOfView;
pub use hand::Hand;
pub use haptics::{Haptics, Vibration, local_haptics};
pub use headset_properties::{EyeImageSize, HeadsetProperties, local_headset_properties};
pub use interaction_profile::InteractionProfile;
#[cfg(all(feature = "openxr", target_os = "android"))]
#[doc(hidden)]
pub use openxr::run;
pub use pose::Pose;
pub use ray::{PanelHit, Ray};
pub use recenter::Recenter;
pub use render_viewpoint::RenderViewpoint;
pub use spatial_capabilities::{SpatialCapabilities, local_spatial_capabilities};
pub use spatial_curved_row::SpatialCurvedRow;
pub use spatial_environment::{SpatialEnvironment, local_spatial_environment};
pub use spatial_external_surface::{SpatialExternalSurface, SpatialExternalSurfaceScope};
pub use spatial_frame_effect::SpatialFrameEffect;
pub use spatial_panel::SpatialPanel;
pub use stereo_mode::StereoMode;
pub use subspace::Subspace;
pub use subspace_modifier::{DEFAULT_PANEL_SIZE, METERS_PER_DP, SubspaceModifier};
pub use xr_frame::XrFrame;
