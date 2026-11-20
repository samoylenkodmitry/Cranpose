//! Cranpose composables for headsets.
//!
//! An app writes a [`Subspace`] of [`SpatialPanel`]s with the same composables as on every
//! other platform. In a headset each panel is a layer of the headset's compositor, which keeps
//! its text sharp. The names follow Jetpack XR Compose. `docs/xr_architecture.md` in the
//! repository describes the design.
//!
//! A [`SpatialVolume`] puts 3D content in a [`Subspace`]: composables whose nodes carry
//! `mesh_surface`, which a headset draws once for each eye into one projection layer with depth,
//! and a window shows in a box. With the `gltf` feature, a `SpatialGltfModel` shows a glTF 2.0
//! model there, with its textures and animations.
//!
//! A [`SpatialExternalSurface`] gives the app a surface that a hardware video decoder or a camera
//! writes into. The compositor shows its pictures as a layer with no copy. A [`StreamedScene`]
//! shows frames that another machine rendered, from the head pose that each was rendered for.
//! Outside a headset it shows the left eye's picture of the newest frame in the layout.
//!
//! A [`SpatialFrameEffect`] gives an app the headset's state in each frame: the head, the eyes,
//! the controllers and the recenter event, as an [`XrFrame`]. An app that asks for it in its build,
//! with `Use::hand_tracking()`, `Use::body_tracking()`, `Use::face_tracking()` or
//! `Use::eye_tracking()` of `cranpose::capabilities`, also gets the user's hands, body, face and
//! gaze there, and [`local_tracking_capabilities`] tells what the runtime offers. A tracked hand
//! with no controller in it points at the panels and presses with a pinch, as a controller does. [`Haptics`] plays
//! vibrations on the controllers. A pure Cranpose app can stream a headset to a host with them.
//!
//! A text field in a panel opens a keyboard when it takes focus, with no code in the app. A headset
//! draws one below the field's panel, or shows the system's where the runtime does, as Android XR
//! does. [`use_keyboard`] chooses.
//!
//! A [`SpatialPointSource`] plays a sound from a pose in a [`Subspace`]. [`SubspaceModifier::point_source`]
//! makes a panel or a volume the source of a sound. The sound follows the head and the source in each
//! frame. Cranpose's own panner in the audio mixer gives each ear its level and its delay, so the
//! runtime has no part in it. The audio player must be Cranpose's engine.
//! [`SpatialCapabilities::is_spatial_audio_enabled`] says whether it is.
//!
//! The `openxr` feature adds the backend for headsets. On Android, as on a Meta Quest, its
//! [`android_main!`] starts the app as an immersive app. On Linux and Windows its `run` starts
//! the app in the headset of the machine's OpenXR runtime (SteamVR, Monado, Meta Link) and opens
//! a flat window when the machine has none. On macOS its `run` opens the flat window. Elsewhere
//! the composables draw flat.
//!
//! The `meta-entitlement` feature adds the entitlement check of the Meta Horizon Store to an
//! Android app. A failed check ends the app unless the app picks [`OnNotEntitled::Continue`].
//! [`local_entitlement_check`] gives the app what the check found. Whoever turns the feature on
//! accepts the Meta Platform Technologies SDK License Agreement, because the app's Gradle build
//! downloads the Meta Platform SDK.

#[cfg(any(test, headset))]
mod aim_filter;
#[cfg(any(test, headset))]
mod beam_pose;
#[cfg(any(test, headset))]
mod billboard_pose;
mod body_joint;
mod body_joint_type;
mod body_state;
mod callback_slot;
#[cfg(any(test, headset))]
mod carry;
mod controller;
#[cfg(any(test, headset))]
mod controller_bindings;
#[cfg(any(test, headset))]
mod depth_order;
mod dock;
mod entitlement;
mod entitlement_check;
#[cfg(any(test, headset))]
mod entitlement_watch;
#[cfg(any(test, headset))]
mod exit_request;
#[cfg(any(test, headset))]
mod extension_offer;
#[cfg(any(test, headset))]
mod eye_camera;
mod eye_state;
mod eyes;
mod face_blend_shape_type;
mod face_confidence_region;
mod face_data_source;
mod face_expression;
mod face_state;
mod face_tracking_provider;
mod field_of_view;
#[cfg(not(target_arch = "wasm32"))]
mod flat_stream;
mod follow_mode;
mod follow_origin;
mod following_subspace;
mod foveated_encoding;
#[cfg(any(test, all(headset, target_os = "android")))]
mod foveated_unwarp;
mod frame_effects;
#[cfg(any(test, all(headset, target_os = "android")))]
mod frame_queue;
#[cfg(feature = "gltf")]
mod gltf_animation;
#[cfg(feature = "gltf")]
mod gltf_buffers;
#[cfg(feature = "gltf")]
mod gltf_clip;
#[cfg(feature = "gltf")]
mod gltf_error;
#[cfg(feature = "gltf")]
mod gltf_fit;
#[cfg(feature = "gltf")]
mod gltf_import;
#[cfg(feature = "gltf")]
mod gltf_material;
#[cfg(feature = "gltf")]
mod gltf_model;
mod hand;
mod hand_aim;
mod hand_joint;
mod hand_joint_type;
mod hand_state;
#[cfg(any(test, headset))]
mod handle;
mod haptics;
mod headset_properties;
mod input_source;
mod interaction_profile;
#[cfg(any(test, headset))]
mod joint_flags;
#[cfg(any(test, headset))]
mod key_layout;
mod keyboard;
#[cfg(any(test, headset))]
mod keyboard_panel;
#[cfg(any(test, headset))]
mod keyboard_state;
#[cfg(any(test, headset))]
mod layer_limit;
mod locals;
mod meta_face_state;
mod move_policy;
mod on_not_entitled;
mod pinch;
mod point_source;
mod point_source_params;
#[cfg(any(test, headset))]
mod pointer;
#[cfg(any(test, headset))]
mod pointer_aim;
mod pose;
mod ray;
mod recenter;
mod render_viewpoint;
#[cfg(any(test, headset))]
mod resize;
mod resize_limits;
mod resize_policy;
#[cfg(any(test, all(feature = "scripted-hand", headset)))]
#[cfg_attr(
    not(all(feature = "scripted-hand", headset)),
    allow(
        unreachable_pub,
        reason = "the tests reach the module, and its API is public only with the feature"
    )
)]
mod scripted_hand;
#[cfg(any(test, headset))]
mod session_status;
#[cfg(any(test, headset))]
mod soft_keyboard;
mod sound_sources;
#[cfg(any(test, headset))]
mod sound_stage;
mod space;
mod spatial_capabilities;
mod spatial_curved_row;
mod spatial_environment;
mod spatial_external_surface;
mod spatial_frame_effect;
#[cfg(feature = "gltf")]
mod spatial_gltf_model;
#[cfg(feature = "gltf")]
mod spatial_gltf_model_source;
#[cfg(feature = "gltf")]
mod spatial_gltf_model_state;
#[cfg(feature = "gltf")]
mod spatial_gltf_model_status;
mod spatial_move_event;
mod spatial_panel;
mod spatial_point_source;
mod spatial_resize_event;
mod spatial_volume;
mod stereo_mode;
mod stream_source;
mod stream_stats;
mod streamed_scene;
mod streamed_view;
mod subspace;
mod subspace_modifier;
mod surface_input;
#[cfg(any(test, headset))]
mod system_support;
mod tracking_availability;
mod tracking_capabilities;
mod tracking_feature;
#[cfg(any(test, headset))]
mod tracking_permission;
#[cfg(any(test, headset))]
mod tracking_request;
mod tracking_state;
#[cfg(any(test, headset))]
mod vector;
#[cfg(any(test, headset))]
mod volume_host;
#[cfg(any(test, headset))]
mod volume_pass;
#[cfg(any(test, headset))]
mod volume_plan;
#[cfg(any(test, headset))]
mod volume_transform;
#[cfg(any(test, headset))]
mod volume_window;
mod volumes;
mod xr_frame;

mod android_main;
#[cfg(all(desktop_run, not(headset)))]
mod flat_run;
#[cfg(headset)]
mod openxr;
#[cfg(desktop_run)]
mod run_error;

pub use body_joint::BodyJoint;
pub use body_joint_type::BodyJointType;
pub use body_state::BodyState;
pub use controller::{Button, Controller, Thumbstick, TrackedPose, Trigger};
#[doc(hidden)]
pub use cranpose as __cranpose;
pub use cranpose::{DistanceAttenuation, ExternalSurface};
pub use entitlement::Entitlement;
pub use entitlement_check::{EntitlementCheck, local_entitlement_check};
pub use eye_state::EyeState;
pub use eyes::Eyes;
pub use face_blend_shape_type::FaceBlendShapeType;
pub use face_confidence_region::FaceConfidenceRegion;
pub use face_data_source::FaceDataSource;
pub use face_expression::FaceExpression;
pub use face_state::FaceState;
pub use face_tracking_provider::FaceTrackingProvider;
pub use field_of_view::FieldOfView;
#[cfg(all(desktop_run, not(headset)))]
pub use flat_run::run;
pub use follow_mode::{FollowMode, FollowTarget};
pub use following_subspace::FollowingSubspace;
pub use foveated_encoding::FoveatedEncoding;
#[cfg(feature = "gltf")]
pub use gltf_animation::{GltfAnimation, GltfAnimationState};
#[cfg(feature = "gltf")]
pub use gltf_error::GltfError;
pub use hand::Hand;
pub use hand_aim::HandAim;
pub use hand_joint::HandJoint;
pub use hand_joint_type::HandJointType;
pub use hand_state::HandState;
pub use haptics::{Haptics, Vibration, local_haptics};
pub use headset_properties::{EyeImageSize, HeadsetProperties, local_headset_properties};
pub use input_source::InputSource;
pub use interaction_profile::InteractionProfile;
pub use keyboard::{Keyboard, use_keyboard};
pub use meta_face_state::MetaFaceState;
pub use move_policy::MovePolicy;
pub use on_not_entitled::OnNotEntitled;
#[cfg(all(headset, target_os = "android"))]
#[doc(hidden)]
pub use openxr::run;
#[cfg(all(headset, not(target_os = "android")))]
pub use openxr::run;
pub use pinch::Pinch;
pub use point_source::PointSource;
pub use point_source_params::PointSourceParams;
pub use pose::Pose;
pub use ray::{PanelHit, Ray};
pub use recenter::Recenter;
pub use render_viewpoint::RenderViewpoint;
pub use resize_limits::ResizeLimits;
pub use resize_policy::ResizePolicy;
#[cfg(desktop_run)]
pub use run_error::RunError;
#[cfg(all(feature = "scripted-hand", headset))]
pub use scripted_hand::{PinchCycle, ScriptedHand, script_hand};
pub use spatial_capabilities::{SpatialCapabilities, local_spatial_capabilities};
pub use spatial_curved_row::SpatialCurvedRow;
pub use spatial_environment::{SpatialEnvironment, local_spatial_environment};
pub use spatial_external_surface::{SpatialExternalSurface, SpatialExternalSurfaceScope};
pub use spatial_frame_effect::SpatialFrameEffect;
#[cfg(feature = "gltf")]
pub use spatial_gltf_model::SpatialGltfModel;
#[cfg(feature = "gltf")]
pub use spatial_gltf_model_source::SpatialGltfModelSource;
#[cfg(feature = "gltf")]
pub use spatial_gltf_model_state::{SpatialGltfModelState, rememberSpatialGltfModelState};
#[cfg(feature = "gltf")]
pub use spatial_gltf_model_status::SpatialGltfModelStatus;
pub use spatial_move_event::{SpatialMoveEvent, SpatialMoveEventType};
pub use spatial_panel::SpatialPanel;
pub use spatial_point_source::SpatialPointSource;
pub use spatial_resize_event::{SpatialResizeEvent, SpatialResizeEventType};
pub use spatial_volume::{SpatialVolume, SpatialVolumeScope};
pub use stereo_mode::StereoMode;
pub use stream_stats::{StreamCounts, StreamStats};
pub use streamed_scene::StreamedScene;
pub use streamed_view::{StreamedView, StreamedViews};
pub use subspace::Subspace;
pub use subspace_modifier::{
    DEFAULT_PANEL_SIZE, DEFAULT_VOLUME_SIZE, METERS_PER_DP, SubspaceModifier,
};
pub use tracking_availability::TrackingAvailability;
pub use tracking_capabilities::{TrackingCapabilities, local_tracking_capabilities};
pub use tracking_feature::TrackingFeature;
pub use tracking_state::TrackingState;
pub use xr_frame::XrFrame;
