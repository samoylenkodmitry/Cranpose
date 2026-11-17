//! The OpenXR instance and session, their events and the ends of the frames.
#![expect(
    unsafe_code,
    reason = "OpenXR's loader, a session on a GL context and layers with extension structs"
)]

use std::{
    ptr,
    time::{Duration, Instant},
};

use android_activity::AndroidApp;
use openxr as xr;

use super::{
    cylinder_layers::{CylinderLayer, raw_layers},
    gl_context::GlContext,
    passthrough::Passthrough,
    refresh_rate,
};
use crate::{
    Fov, Pose,
    stereo_mode::{Eye, EyeView},
};

const VIEW_CONFIGURATION: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;
// How long the app waits for the runtime to stop a running session as the app ends, and how
// often it asks. An activity that is destroyed has few seconds before Android stops it.
const STOP_WAIT: Duration = Duration::from_millis(1000);
const STOP_POLL: Duration = Duration::from_millis(10);

// Where the session stands after its events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionStatus {
    // The session has not begun or has ended: the app waits.
    Idle,
    // The session runs and the runtime shows none of its layers: the app ends frames with no
    // layers.
    Hidden,
    // The runtime shows the app's layers, with something over them such as the system menu. The
    // app gets no input.
    Visible,
    // The runtime shows the app's layers and sends the headset's input to the app.
    Focused,
    // The runtime ends the session: the app ends.
    Exit,
}

impl SessionStatus {
    // Whether the session runs: the app begins and ends frames.
    pub(crate) fn runs(self) -> bool {
        matches!(self, Self::Hidden | Self::Visible | Self::Focused)
    }

    // Whether the runtime shows the app's layers.
    pub(crate) fn shows(self) -> bool {
        matches!(self, Self::Visible | Self::Focused)
    }

    // Whether the app gets the controllers' input.
    pub(crate) fn listens(self) -> bool {
        self == Self::Focused
    }
}

// What a layer shows of its swapchain.
#[derive(Clone, Copy)]
pub(crate) enum LayerSource {
    // The whole image of this size in pixels, which wgpu drew, to both eyes. The compositor
    // blends its alpha.
    Drawn((u32, u32)),
    // The part of a picture that a decoder or a camera writes into a surface swapchain. The
    // picture is opaque.
    Surface(EyeView),
}

// One eye of a projection layer: a swapchain image, drawn from the pose and with the field of
// view that the layer says.
pub(crate) struct ProjectionEye<'a> {
    pub(crate) swapchain: &'a xr::Swapchain<xr::OpenGlEs>,
    // The image's size in pixels. The eye shows all of it.
    pub(crate) size: (u32, u32),
    pub(crate) pose: Pose,
    pub(crate) fov: Fov,
}

// A layer that shows the pictures that were rendered for both eyes, from the head poses that
// they were rendered for. The compositor moves them to the head's newer pose.
pub(crate) struct ProjectionLayer<'a> {
    pub(crate) eyes: [ProjectionEye<'a>; 2],
}

// A layer that shows a swapchain image on a flat panel.
pub(crate) struct QuadLayer<'a> {
    pub(crate) swapchain: &'a xr::Swapchain<xr::OpenGlEs>,
    pub(crate) source: LayerSource,
    pub(crate) pose: Pose,
    // The panel's width and height in meters.
    pub(crate) size: [f32; 2],
}

// The fields drop in their order: the spaces and the frame ends, then the session, then the
// instance.
pub(crate) struct XrSession {
    // The room around the panels. Its handles end before the session does, so it comes first.
    pub(crate) passthrough: Passthrough,
    // The room as the app sees it: its origin is where the head was when the app started
    pub(crate) local: xr::Space,
    // The head's own space, which the runtime locates for each frame
    view: xr::Space,
    waiter: xr::FrameWaiter,
    stream: xr::FrameStream<xr::OpenGlEs>,
    pub(crate) session: xr::Session<xr::OpenGlEs>,
    instance: xr::Instance,
    // Whether the runtime shows cylinder layers, which curved panels are
    cylinders: bool,
    // Whether layers say their images start with the top row, which is how wgpu writes them
    flips_rows: bool,
    status: SessionStatus,
}

impl XrSession {
    // Starts OpenXR for this app's activity and makes a session that draws on `gl`. The
    // session must end before `gl` does.
    pub(crate) fn new(gl: &GlContext, app: &AndroidApp) -> Result<Self, String> {
        // The runtime finds the app's window through the activity it is given. The process's
        // NDK context can hold the application context instead, and with it the runtime shows
        // none of the app's frames.
        // SAFETY: the Java VM and the global reference to the activity live as long as the
        // AndroidApp, which outlives the session.
        let platform =
            unsafe { xr::AndroidPlatformInfo::new(app.vm_as_ptr(), app.activity_as_ptr()) };
        // SAFETY: the library is the Khronos OpenXR loader, which the `xr` service of the
        // Gradle plugin packs into the APK.
        let entry = unsafe { xr::Entry::load(&platform) }
            .map_err(|error| format!("the OpenXR loader: {error}"))?;
        let available = entry
            .enumerate_extensions()
            .map_err(|error| format!("OpenXR extensions: {error}"))?;
        if !available.khr_opengl_es_enable {
            return Err("the OpenXR runtime has no OpenGL ES".into());
        }
        let extensions = wanted_extensions(&available);
        log::info!(
            "cranpose xr: cylinder layers {}, passthrough extension {}",
            extensions.khr_composition_layer_cylinder,
            extensions.fb_passthrough,
        );
        if !available.fb_composition_layer_image_layout {
            log::warn!("cranpose xr: the runtime cannot flip layers; panels show upside down");
        }
        let instance = entry
            .create_instance(
                &xr::ApplicationInfo {
                    application_name: "cranpose",
                    application_version: 0,
                    engine_name: "Cranpose",
                    engine_version: 0,
                    api_version: xr::Version::new(1, 0, 0),
                },
                &extensions,
                &[],
                &platform,
            )
            .map_err(|error| format!("OpenXR instance: {error}"))?;
        let system = instance
            .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
            .map_err(|error| format!("no headset: {error}"))?;
        log_runtime(&instance, system);
        // OpenXR asks for the requirements before a session; GLES 3 meets every runtime's
        instance
            .graphics_requirements::<xr::OpenGlEs>(system)
            .map_err(|error| format!("OpenXR GLES requirements: {error}"))?;
        let (display, config, context) = gl.handles();
        // SAFETY: the handles are the current EGL context's, which outlives the session.
        let (session, waiter, stream) = unsafe {
            instance.create_session::<xr::OpenGlEs>(
                system,
                &xr::opengles::SessionCreateInfo::Android {
                    display,
                    config,
                    context,
                },
            )
        }
        .map_err(|error| format!("OpenXR session: {error}"))?;
        let local = session
            .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            .map_err(|error| format!("OpenXR space: {error}"))?;
        let view = session
            .create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)
            .map_err(|error| format!("OpenXR view space: {error}"))?;
        let modes = instance
            .enumerate_environment_blend_modes(system, VIEW_CONFIGURATION)
            .unwrap_or_default();
        let passthrough = Passthrough::new(&session, &modes);
        Ok(Self {
            passthrough,
            local,
            view,
            waiter,
            stream,
            session,
            instance,
            cylinders: extensions.khr_composition_layer_cylinder,
            flips_rows: available.fb_composition_layer_image_layout,
            status: SessionStatus::Idle,
        })
    }

    pub(crate) fn instance(&self) -> &xr::Instance {
        &self.instance
    }

    // Whether the runtime shows curved panels on cylinder layers. If not, they show flat.
    pub(crate) fn shows_cylinders(&self) -> bool {
        self.cylinders
    }

    // Whether layers show their images from the top row, which is how wgpu writes them. If not,
    // the first row is the bottom one.
    pub(crate) fn flips_rows(&self) -> bool {
        self.flips_rows
    }

    // The head at `time`, in the `local` space. A head the runtime does not track has none.
    pub(crate) fn head(&self, time: xr::Time) -> Option<Pose> {
        let location = self.view.locate(&self.local, time).ok()?;
        let tracked =
            xr::SpaceLocationFlags::POSITION_VALID | xr::SpaceLocationFlags::ORIENTATION_VALID;
        location
            .location_flags
            .contains(tracked)
            .then(|| pose_of(&location.pose))
    }

    // Handles the runtime's events: starts and stops the session as the headset asks, and
    // returns where the session stands.
    pub(crate) fn poll_events(&mut self) -> Result<SessionStatus, String> {
        let mut buffer = xr::EventDataBuffer::new();
        while let Some(event) = self
            .instance
            .poll_event(&mut buffer)
            .map_err(|error| format!("OpenXR events: {error}"))?
        {
            match event {
                xr::Event::SessionStateChanged(change) => self.on_state(change.state())?,
                xr::Event::DisplayRefreshRateChangedFB(change) => log::info!(
                    "cranpose xr: refresh rate {} Hz to {} Hz",
                    change.from_display_refresh_rate(),
                    change.to_display_refresh_rate()
                ),
                xr::Event::InstanceLossPending(_) => {
                    log::info!("cranpose xr: the runtime loses the instance");
                    self.status = SessionStatus::Exit;
                }
                _ => {}
            }
            if self.status == SessionStatus::Exit {
                break;
            }
        }
        Ok(self.status)
    }

    // Follows the session into `state`. The runtime moves it from READY through SYNCHRONIZED and
    // VISIBLE to FOCUSED, and back down when something else takes the headset.
    fn on_state(&mut self, state: xr::SessionState) -> Result<(), String> {
        log::info!("cranpose xr: session {state:?}");
        self.status = match state {
            xr::SessionState::READY => {
                self.session
                    .begin(VIEW_CONFIGURATION)
                    .map_err(|error| format!("OpenXR begin: {error}"))?;
                if self.instance.exts().fb_display_refresh_rate.is_some() {
                    refresh_rate::request_highest(&self.session);
                }
                SessionStatus::Hidden
            }
            xr::SessionState::SYNCHRONIZED => SessionStatus::Hidden,
            xr::SessionState::VISIBLE => SessionStatus::Visible,
            xr::SessionState::FOCUSED => SessionStatus::Focused,
            xr::SessionState::STOPPING => {
                self.session
                    .end()
                    .map_err(|error| format!("OpenXR end: {error}"))?;
                SessionStatus::Idle
            }
            xr::SessionState::IDLE => SessionStatus::Idle,
            xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => SessionStatus::Exit,
            _ => self.status,
        };
        Ok(())
    }

    // Waits for the runtime's next frame and begins it.
    pub(crate) fn begin_frame(&mut self) -> Result<xr::FrameState, String> {
        let state = self
            .waiter
            .wait()
            .map_err(|error| format!("OpenXR wait frame: {error}"))?;
        self.stream
            .begin()
            .map_err(|error| format!("OpenXR begin frame: {error}"))?;
        Ok(state)
    }

    // Ends the frame begun last. The room is the bottom layer while the runtime shows the app's
    // layers. Then come the streamed scenes, the curved panels, and `layers`, back to front.
    pub(crate) fn end_frame(
        &mut self,
        display_time: xr::Time,
        streams: &[ProjectionLayer<'_>],
        curved: &[CylinderLayer<'_>],
        layers: &[QuadLayer<'_>],
    ) -> Result<(), String> {
        let mut layouts: Vec<xr::sys::CompositionLayerImageLayoutFB> = layers
            .iter()
            .map(|_| xr::sys::CompositionLayerImageLayoutFB {
                ty: xr::sys::CompositionLayerImageLayoutFB::TYPE,
                next: ptr::null_mut(),
                flags: xr::sys::CompositionLayerImageLayoutFlagsFB::VERTICAL_FLIP,
            })
            .collect();
        let quads: Vec<xr::CompositionLayerQuad<'_, xr::OpenGlEs>> = layers
            .iter()
            .zip(layouts.iter_mut())
            .map(|(layer, layout)| {
                let (eye_visibility, offset, extent, blends) = match layer.source {
                    LayerSource::Drawn((width, height)) => {
                        (xr::EyeVisibility::BOTH, [0, 0], [width, height], true)
                    }
                    LayerSource::Surface(view) => {
                        (eye_visibility(view.eye), view.offset, view.extent, false)
                    }
                };
                let raw = xr::sys::CompositionLayerQuad {
                    ty: xr::sys::CompositionLayerQuad::TYPE,
                    // The runtime reads the first row of any swapchain as the bottom one. The
                    // images of wgpu and the pictures of a surface's producer start with the top
                    // row.
                    next: if self.flips_rows {
                        ptr::from_mut(layout).cast_const().cast()
                    } else {
                        ptr::null()
                    },
                    layer_flags: if blends {
                        xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA
                    } else {
                        xr::CompositionLayerFlags::EMPTY
                    },
                    space: self.local.as_raw(),
                    eye_visibility,
                    sub_image: xr::sys::SwapchainSubImage {
                        swapchain: layer.swapchain.as_raw(),
                        image_rect: xr::Rect2Di {
                            offset: xr::Offset2Di {
                                x: offset[0] as i32,
                                y: offset[1] as i32,
                            },
                            extent: xr::Extent2Di {
                                width: extent[0] as i32,
                                height: extent[1] as i32,
                            },
                        },
                        image_array_index: 0,
                    },
                    pose: posef(&layer.pose),
                    size: xr::Extent2Df {
                        width: layer.size[0],
                        height: layer.size[1],
                    },
                };
                // SAFETY: the struct names a live space and swapchain, and its next chain holds
                // a layout struct that outlives the frame's end below.
                unsafe { xr::CompositionLayerQuad::from_raw(raw) }
            })
            .collect();
        let flip = xr::sys::CompositionLayerImageLayoutFB {
            ty: xr::sys::CompositionLayerImageLayoutFB::TYPE,
            next: ptr::null_mut(),
            flags: xr::sys::CompositionLayerImageLayoutFlagsFB::VERTICAL_FLIP,
        };
        let cylinders = raw_layers(curved, &self.local, self.flips_rows.then_some(&flip));
        let views: Vec<[xr::CompositionLayerProjectionView<'_, xr::OpenGlEs>; 2]> =
            streams.iter().map(projection_views).collect();
        let projections: Vec<xr::CompositionLayerProjection<'_, xr::OpenGlEs>> = views
            .iter()
            .map(|views| {
                xr::CompositionLayerProjection::new()
                    .layer_flags(xr::CompositionLayerFlags::EMPTY)
                    .space(&self.local)
                    .views(views)
            })
            .collect();
        let references: Vec<&xr::CompositionLayerBase<'_, xr::OpenGlEs>> = self
            .passthrough
            .layer()
            .filter(|_| self.status.shows())
            .into_iter()
            .chain(projections.iter().map(|projection| &**projection))
            .chain(cylinders.iter().map(|cylinder| &**cylinder))
            .chain(quads.iter().map(|quad| &**quad))
            .collect();
        self.stream
            .end(display_time, self.passthrough.blend_mode(), &references)
            .map_err(|error| format!("OpenXR end frame: {error}"))
    }
}

// A session that still runs when the app ends goes through the runtime's own steps: the app asks
// to exit, the runtime moves it to STOPPING, and the app ends it. The session and the instance
// drop after that.
impl Drop for XrSession {
    fn drop(&mut self) {
        if !self.status.runs() {
            return;
        }
        if let Err(error) = self.session.request_exit() {
            log::warn!("cranpose xr: exit request: {error}");
            return;
        }
        let deadline = Instant::now() + STOP_WAIT;
        while self.status.runs() && Instant::now() < deadline {
            if self.poll_events().is_err() {
                break;
            }
            std::thread::sleep(STOP_POLL);
        }
        log::info!("cranpose xr: session ends in {:?}", self.status);
    }
}

// The extensions the app asks for, from those the runtime has.
fn wanted_extensions(available: &xr::ExtensionSet) -> xr::ExtensionSet {
    let mut extensions = xr::ExtensionSet::default();
    extensions.khr_opengl_es_enable = true;
    extensions.khr_android_create_instance = true;
    extensions.fb_composition_layer_image_layout = available.fb_composition_layer_image_layout;
    // Curved panels show on cylinder layers
    extensions.khr_composition_layer_cylinder = available.khr_composition_layer_cylinder;
    // The room around the panels shows through a passthrough layer
    extensions.fb_passthrough = available.fb_passthrough;
    // A decoder or a camera writes into a surface that the compositor shows with no copy
    extensions.khr_android_surface_swapchain = available.khr_android_surface_swapchain;
    // The highest refresh rate the runtime offers
    extensions.fb_display_refresh_rate = available.fb_display_refresh_rate;
    // Hands tracked by cameras point and pinch as controllers do. The controllers of other
    // makers bind through their own profiles, each behind an extension.
    extensions.ext_hand_interaction = available.ext_hand_interaction;
    extensions.khr_generic_controller = available.khr_generic_controller;
    extensions.htc_vive_focus3_controller_interaction =
        available.htc_vive_focus3_controller_interaction;
    extensions.htc_vive_cosmos_controller_interaction =
        available.htc_vive_cosmos_controller_interaction;
    extensions.ext_hp_mixed_reality_controller = available.ext_hp_mixed_reality_controller;
    extensions.ext_samsung_odyssey_controller = available.ext_samsung_odyssey_controller;
    extensions.bd_controller_interaction = available.bd_controller_interaction;
    extensions.ml_ml2_controller_interaction = available.ml_ml2_controller_interaction;
    extensions.meta_touch_controller_plus = available.meta_touch_controller_plus;
    extensions.fb_touch_controller_pro = available.fb_touch_controller_pro;
    extensions
}

// Names the runtime and the headset once in the log, so a report says which runtime it came from.
fn log_runtime(instance: &xr::Instance, system: xr::SystemId) {
    match instance.properties() {
        Ok(runtime) => log::info!(
            "cranpose xr: runtime \"{}\" {}",
            runtime.runtime_name,
            runtime.runtime_version
        ),
        Err(error) => log::warn!("cranpose xr: runtime properties: {error}"),
    }
    match instance.system_properties(system) {
        Ok(headset) => log::info!(
            "cranpose xr: system \"{}\", vendor {:#x}, swapchains up to {} by {} pixels",
            headset.system_name,
            headset.vendor_id,
            headset.graphics_properties.max_swapchain_image_width,
            headset.graphics_properties.max_swapchain_image_height
        ),
        Err(error) => log::warn!("cranpose xr: system properties: {error}"),
    }
}

// The views of a projection layer, left then right. Each shows all of its image.
fn projection_views<'a>(
    layer: &ProjectionLayer<'a>,
) -> [xr::CompositionLayerProjectionView<'a, xr::OpenGlEs>; 2] {
    layer.eyes.each_ref().map(|eye| {
        xr::CompositionLayerProjectionView::new()
            .pose(posef(&eye.pose))
            .fov(xr::Fovf {
                angle_left: eye.fov.angle_left,
                angle_right: eye.fov.angle_right,
                angle_up: eye.fov.angle_up,
                angle_down: eye.fov.angle_down,
            })
            .sub_image(
                xr::SwapchainSubImage::new()
                    .swapchain(eye.swapchain)
                    .image_rect(xr::Rect2Di {
                        offset: xr::Offset2Di { x: 0, y: 0 },
                        extent: xr::Extent2Di {
                            width: eye.size.0 as i32,
                            height: eye.size.1 as i32,
                        },
                    }),
            )
    })
}

fn eye_visibility(eye: Eye) -> xr::EyeVisibility {
    match eye {
        Eye::Both => xr::EyeVisibility::BOTH,
        Eye::Left => xr::EyeVisibility::LEFT,
        Eye::Right => xr::EyeVisibility::RIGHT,
    }
}

pub(crate) fn posef(pose: &Pose) -> xr::Posef {
    let [x, y, z] = pose.position;
    let [qx, qy, qz, qw] = pose.orientation;
    xr::Posef {
        orientation: xr::Quaternionf {
            x: qx,
            y: qy,
            z: qz,
            w: qw,
        },
        position: xr::Vector3f { x, y, z },
    }
}

pub(crate) fn pose_of(pose: &xr::Posef) -> Pose {
    Pose {
        position: [pose.position.x, pose.position.y, pose.position.z],
        orientation: [
            pose.orientation.x,
            pose.orientation.y,
            pose.orientation.z,
            pose.orientation.w,
        ],
    }
}
