//! Cranpose composables for headsets.
//!
//! An app writes a [`Subspace`] of [`SpatialPanel`]s with the same composables as on every
//! other platform. In a headset each panel is a layer of the headset's compositor, which keeps
//! its text sharp. The names follow Jetpack XR Compose. `docs/xr_architecture.md` in the
//! repository describes the design.
//!
//! A [`SpatialExternalSurface`] gives the app a surface that a hardware video decoder or a camera
//! writes into. The compositor shows its pictures as a layer with no copy. A [`StreamedScene`]
//! shows frames that another machine rendered, from the head pose that each was rendered for.
//!
//! A [`SpatialFrameEffect`] gives an app the headset's state in each frame: the head, the eyes,
//! the controllers and the recenter event, as an [`XrFrame`]. [`Haptics`] plays vibrations on the
//! controllers. A pure Cranpose app can stream a headset to a host with them.
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
mod controller;
#[cfg(any(test, headset))]
mod controller_bindings;
#[cfg(any(test, headset))]
mod depth_order;
mod field_of_view;
mod foveated_encoding;
#[cfg(any(test, all(headset, target_os = "android")))]
mod foveated_unwarp;
mod frame_effects;
#[cfg(any(test, all(headset, target_os = "android")))]
mod frame_queue;
mod hand;
mod haptics;
mod headset_properties;
mod interaction_profile;
mod locals;
mod move_policy;
#[cfg(any(test, headset))]
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
mod spatial_move_event;
mod spatial_panel;
mod stereo_mode;
mod stream_source;
mod stream_stats;
mod streamed_scene;
mod streamed_view;
mod subspace;
mod subspace_modifier;
mod surface_input;
mod xr_frame;

mod android_main;
#[cfg(all(desktop_run, not(headset)))]
mod flat_run;
#[cfg(headset)]
mod openxr;
#[cfg(desktop_run)]
mod run_error;

pub use controller::{Button, Controller, Thumbstick, TrackedPose, Trigger};
#[doc(hidden)]
pub use cranpose as __cranpose;
pub use cranpose::ExternalSurface;
pub use field_of_view::FieldOfView;
#[cfg(all(desktop_run, not(headset)))]
pub use flat_run::run;
pub use foveated_encoding::FoveatedEncoding;
pub use hand::Hand;
pub use haptics::{Haptics, Vibration, local_haptics};
pub use headset_properties::{EyeImageSize, HeadsetProperties, local_headset_properties};
pub use interaction_profile::InteractionProfile;
pub use move_policy::MovePolicy;
#[cfg(all(headset, target_os = "android"))]
#[doc(hidden)]
pub use openxr::run;
#[cfg(all(headset, not(target_os = "android")))]
pub use openxr::run;
pub use pose::Pose;
pub use ray::{PanelHit, Ray};
pub use recenter::Recenter;
pub use render_viewpoint::RenderViewpoint;
#[cfg(desktop_run)]
pub use run_error::RunError;
pub use spatial_capabilities::{SpatialCapabilities, local_spatial_capabilities};
pub use spatial_curved_row::SpatialCurvedRow;
pub use spatial_environment::{SpatialEnvironment, local_spatial_environment};
pub use spatial_external_surface::{SpatialExternalSurface, SpatialExternalSurfaceScope};
pub use spatial_frame_effect::SpatialFrameEffect;
pub use spatial_move_event::{SpatialMoveEvent, SpatialMoveEventType};
pub use spatial_panel::SpatialPanel;
pub use stereo_mode::StereoMode;
pub use stream_stats::{StreamCounts, StreamStats};
pub use streamed_scene::StreamedScene;
pub use streamed_view::{StreamedView, StreamedViews};
pub use subspace::Subspace;
pub use subspace_modifier::{DEFAULT_PANEL_SIZE, METERS_PER_DP, SubspaceModifier};
pub use xr_frame::XrFrame;
