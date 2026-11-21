//! The OpenXR session, its events and the ends of the frames.
#![expect(
    unsafe_code,
    reason = "the list of raw layers that the runtime reads through pointers"
)]

use std::{
    ffi::c_void,
    ptr, slice,
    time::{Duration, Instant},
};

use openxr as xr;

use super::{
    frame_layers::{Chains, Counts, FrameLayers, Layer, Stages},
    graphics::{Api, Graphics},
    passthrough::Passthrough,
    recenter_events::RecenterEvents,
    refresh_rate,
    runtime::Runtime,
    spatial::SpatialEvents,
    tracked_pose::tracked,
};
#[cfg(target_os = "android")]
use crate::stereo_mode::{Eye, EyeView};
use crate::{
    FieldOfView, Pose, TrackedPose,
    layer_limit::LayerLimit,
    session_status::{SessionState, SessionStatus},
    volume_plan::VolumePlan,
};

const VIEW_CONFIGURATION: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;
// How long the app waits for the runtime to stop a running session as the app ends, and how
// often it asks. An activity that is destroyed has few seconds before Android stops it.
const STOP_WAIT: Duration = Duration::from_millis(1000);
const STOP_POLL: Duration = Duration::from_millis(10);

// What a layer shows of its swapchain.
#[derive(Clone, Copy)]
pub(crate) enum LayerSource {
    // The whole image of this size in pixels, which wgpu drew, to both eyes. The compositor
    // blends its alpha.
    Drawn((u32, u32)),
    // The part of a picture that a decoder or a camera writes into a surface swapchain. The
    // picture is opaque. Only an Android headset's compositor shows surfaces.
    #[cfg(target_os = "android")]
    Surface(EyeView),
}

// The far plane that a layer's depth names, in meters. The scene's depth is that of a far plane at
// infinity, which the extension allows. A finite plane this far away differs from it by a part in
// a million, and a runtime has no infinity to mishandle.
const DEPTH_FAR_Z: f32 = 10_000.0;

// The depth of one eye's picture: a swapchain image that holds `1 - near / z` for a point at
// distance `z` along the eye's axis, which is the depth of a camera whose far plane is infinite.
pub(crate) struct EyeDepth<'a> {
    pub(crate) swapchain: &'a xr::Swapchain<Api>,
    // The distance of the near plane in meters
    pub(crate) near: f32,
}

// One eye of a projection layer: a swapchain image, drawn from the pose and with the field of
// view that the layer says.
pub(crate) struct ProjectionEye<'a> {
    pub(crate) swapchain: &'a xr::Swapchain<Api>,
    // The image's size in pixels. The eye shows all of it.
    pub(crate) size: (u32, u32),
    pub(crate) pose: Pose,
    pub(crate) fov: FieldOfView,
    // The depth of the picture, if the layer carries it
    pub(crate) depth: Option<EyeDepth<'a>>,
}

impl ProjectionEye<'_> {
    // The rectangle of an image that the eye shows: all of it.
    fn image_rect(&self) -> xr::Rect2Di {
        xr::Rect2Di {
            offset: xr::Offset2Di { x: 0, y: 0 },
            extent: xr::Extent2Di {
                width: self.size.0 as i32,
                height: self.size.1 as i32,
            },
        }
    }

    // The view of the eye: all of its image.
    fn to_raw(&self) -> xr::sys::CompositionLayerProjectionView {
        xr::CompositionLayerProjectionView::<Api>::new()
            .pose(posef(&self.pose))
            .fov(fovf(&self.fov))
            .sub_image(
                xr::SwapchainSubImage::new()
                    .swapchain(self.swapchain)
                    .image_rect(self.image_rect()),
            )
            .into_raw()
    }

    // The struct that ties the eye's depth to its view, which the view's `next` pointer must name.
    fn depth_to_raw(&self) -> Option<xr::sys::CompositionLayerDepthInfoKHR> {
        let depth = self.depth.as_ref()?;
        Some(xr::sys::CompositionLayerDepthInfoKHR {
            ty: xr::sys::CompositionLayerDepthInfoKHR::TYPE,
            next: ptr::null(),
            sub_image: xr::sys::SwapchainSubImage {
                swapchain: depth.swapchain.as_raw(),
                image_rect: self.image_rect(),
                image_array_index: 0,
            },
            min_depth: 0.0,
            max_depth: 1.0,
            near_z: depth.near,
            far_z: DEPTH_FAR_Z,
        })
    }
}

// Whether the compositor reads a layer's picture as opaque or blends it with what lies under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Alpha {
    // The picture covers what lies under it
    #[cfg_attr(
        not(target_os = "android"),
        expect(
            dead_code,
            reason = "only the streamed scenes of an Android headset use it"
        )
    )]
    Ignored,
    // The picture's color is premultiplied with its alpha, and its alpha says how much of what
    // lies under it shows
    Premultiplied,
}

// A layer that shows the pictures that were rendered for both eyes, from the head poses that
// they were rendered for. The compositor moves them to the head's newer pose.
pub(crate) struct ProjectionLayer<'a> {
    pub(crate) eyes: [ProjectionEye<'a>; 2],
    pub(crate) alpha: Alpha,
}

// The structs of a projection layer's views and of their depth, which the layer's `views` pointer
// and each view's `next` pointer name.
pub(crate) struct RawProjection {
    pub(crate) layer: xr::sys::CompositionLayerProjection,
    pub(crate) views: [xr::sys::CompositionLayerProjectionView; 2],
    pub(crate) depths: [Option<xr::sys::CompositionLayerDepthInfoKHR>; 2],
}

impl ProjectionLayer<'_> {
    // The compositor's structs for the layer in `space`. They name the swapchains by their
    // handles, so they are valid for as long as the swapchains live. The pointers between the
    // structs are set when the structs have their place in the frame's list.
    pub(crate) fn to_raw(&self, space: xr::sys::Space) -> RawProjection {
        let layer = xr::sys::CompositionLayerProjection {
            ty: xr::sys::CompositionLayerProjection::TYPE,
            next: ptr::null(),
            layer_flags: match self.alpha {
                Alpha::Ignored => xr::CompositionLayerFlags::EMPTY,
                Alpha::Premultiplied => xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA,
            },
            space,
            view_count: 2,
            views: ptr::null(),
        };
        RawProjection {
            layer,
            views: self.eyes.each_ref().map(ProjectionEye::to_raw),
            depths: self.eyes.each_ref().map(ProjectionEye::depth_to_raw),
        }
    }
}

// A layer that shows a swapchain image on a flat panel.
pub(crate) struct QuadLayer<'a> {
    pub(crate) swapchain: &'a xr::Swapchain<Api>,
    pub(crate) source: LayerSource,
    pub(crate) pose: Pose,
    // The panel's width and height in meters.
    pub(crate) size: [f32; 2],
}

impl QuadLayer<'_> {
    // The compositor's struct for the layer in `space`. It names the swapchain by its handle, so
    // the struct is valid for as long as the swapchain lives.
    pub(crate) fn to_raw(&self, space: xr::sys::Space) -> xr::sys::CompositionLayerQuad {
        let (eye_visibility, offset, extent, blends) = match self.source {
            LayerSource::Drawn((width, height)) => {
                (xr::EyeVisibility::BOTH, [0_u32, 0], [width, height], true)
            }
            #[cfg(target_os = "android")]
            LayerSource::Surface(view) => {
                (eye_visibility(view.eye), view.offset, view.extent, false)
            }
        };
        xr::sys::CompositionLayerQuad {
            ty: xr::sys::CompositionLayerQuad::TYPE,
            next: ptr::null(),
            layer_flags: if blends {
                xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA
            } else {
                xr::CompositionLayerFlags::EMPTY
            },
            space,
            eye_visibility,
            sub_image: xr::sys::SwapchainSubImage {
                swapchain: self.swapchain.as_raw(),
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
            pose: posef(&self.pose),
            size: xr::Extent2Df {
                width: self.size[0],
                height: self.size[1],
            },
        }
    }
}

// The fields drop in their order: the spaces and the frame ends, then the session, then the
// instance.
pub(crate) struct XrSession {
    // The room around the panels. Its handles end before the session does, so it comes first.
    pub(crate) passthrough: Passthrough,
    // The runtime's recenter events that no frame has reported yet
    pub(crate) recenters: RecenterEvents,
    // The runtime's events about spatial entities that no frame has read yet
    pub(crate) spatial_events: SpatialEvents,
    // The room as the app sees it: its origin is where the head was when the app started
    pub(crate) local: xr::Space,
    // The head's own space, which the runtime locates for each frame
    view: xr::Space,
    // The floor: the runtime's STAGE space, when it has one
    stage: Option<xr::Space>,
    waiter: xr::FrameWaiter,
    stream: xr::FrameStream<Api>,
    pub(crate) session: xr::Session<Api>,
    instance: xr::Instance,
    // Whether the runtime shows cylinder layers, which curved panels are
    cylinders: bool,
    // Whether a projection layer carries depth, and whether the layers over it test against it
    layer_depth: bool,
    depth_test: bool,
    // Whether layers say that their images start with the top row, because the API's own first
    // row is the bottom one and the runtime can flip it
    flips_layers: bool,
    status: SessionStatus,
    // The raw layers of the frame that ends. The lists keep their capacity from frame to frame.
    layers: FrameLayers,
    // What the last logged frame held, so that the log names each change once
    logged_layers: Counts,
    // The most layers that the runtime takes in a frame
    limit: LayerLimit,
}

impl XrSession {
    // Makes a session on the device of `graphics`, which must outlive the session.
    pub(crate) fn new(runtime: Runtime, graphics: &Graphics) -> Result<Self, String> {
        let Runtime {
            instance,
            system,
            flips_layers,
            cylinders,
            layer_depth,
            depth_test,
            tracking: _,
            spatial: _,
            render_models: _,
        } = runtime;
        if !Graphics::TOP_ROW_FIRST && !flips_layers {
            log::warn!("cranpose xr: the runtime cannot flip layers; panels show upside down");
        }
        let (session, waiter, stream) = graphics.create_session(&instance, system)?;
        let local = session
            .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            .map_err(|error| format!("OpenXR space: {error}"))?;
        let local_handle = local.as_raw();
        let view = session
            .create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)
            .map_err(|error| format!("OpenXR view space: {error}"))?;
        // A runtime that has no stage rejects the space, so the app asks only for what it lists
        let has_stage = session
            .enumerate_reference_spaces()
            .is_ok_and(|spaces| spaces.contains(&xr::ReferenceSpaceType::STAGE));
        let stage = if has_stage {
            session
                .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
                .map_err(|error| log::warn!("cranpose xr: no stage space: {error}"))
                .ok()
        } else {
            None
        };
        let modes = instance
            .enumerate_environment_blend_modes(system, VIEW_CONFIGURATION)
            .unwrap_or_default();
        let passthrough = Passthrough::new(&session, &modes);
        let limit =
            LayerLimit::reported(instance.system_properties(system).map_or(0, |properties| {
                properties.graphics_properties.max_layer_count
            }));
        Ok(Self {
            passthrough,
            recenters: RecenterEvents::default(),
            spatial_events: SpatialEvents::default(),
            local,
            view,
            stage,
            waiter,
            stream,
            session,
            instance,
            cylinders,
            layer_depth,
            depth_test,
            flips_layers: flips_layers && !Graphics::TOP_ROW_FIRST,
            status: SessionStatus::Idle,
            layers: FrameLayers::new(local_handle),
            logged_layers: Counts::default(),
            limit,
        })
    }

    pub(crate) fn instance(&self) -> &xr::Instance {
        &self.instance
    }

    // Whether the runtime shows curved panels on cylinder layers. If not, they show flat.
    pub(crate) fn shows_cylinders(&self) -> bool {
        self.cylinders
    }

    // What the runtime offers for the depth of a projection layer: whether the layer carries it,
    // and whether the layers over it test against it.
    pub(crate) fn depth_extensions(&self) -> (bool, bool) {
        (self.layer_depth, self.depth_test)
    }

    // Whether layers show their images from the top row, which is how wgpu writes them. If not,
    // the first row is the bottom one.
    pub(crate) fn top_row_first(&self) -> bool {
        Graphics::TOP_ROW_FIRST || self.flips_layers
    }

    // The head at `time`, in the `local` space, with its velocities. A head that the runtime does
    // not track has none.
    pub(crate) fn head(&self, time: xr::Time) -> Option<TrackedPose> {
        tracked(&self.view, &self.local, time)
    }

    // The pose of the stage at `time` in the `local` space: the floor at the center of the play
    // area. A runtime with no stage, or one that does not locate it now, gives `None`.
    pub(crate) fn stage(&self, time: xr::Time) -> Option<Pose> {
        let stage = self.stage.as_ref()?;
        tracked(stage, &self.local, time).map(|located| located.pose)
    }

    // Asks the runtime to end the running session. The runtime moves it to STOPPING, which
    // `poll_events` ends.
    pub(crate) fn request_exit(&self) -> Result<(), String> {
        self.session
            .request_exit()
            .map_err(|error| format!("OpenXR request exit: {error}"))
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
                xr::Event::ReferenceSpaceChangePending(change) => {
                    self.recenters.record(&self.instance, change);
                }
                xr::Event::InstanceLossPending(_) => {
                    log::info!("cranpose xr: the runtime loses the instance");
                    self.status = SessionStatus::Exit;
                }
                other => {
                    self.spatial_events.record(&other);
                }
            }
            if self.status == SessionStatus::Exit {
                break;
            }
        }
        Ok(self.status)
    }

    // Follows the session into `state`. The runtime moves it from READY through SYNCHRONIZED and
    // VISIBLE to FOCUSED, and back down when something else takes the headset. A state that
    // this backend does not know leaves the status as it was.
    fn on_state(&mut self, state: xr::SessionState) -> Result<(), String> {
        log::info!("cranpose xr: session {state:?}");
        match state {
            xr::SessionState::READY => {
                self.session
                    .begin(VIEW_CONFIGURATION)
                    .map_err(|error| format!("OpenXR begin: {error}"))?;
                refresh_rate::request_highest(&self.instance, &self.session);
            }
            xr::SessionState::STOPPING => {
                self.session
                    .end()
                    .map_err(|error| format!("OpenXR end: {error}"))?;
            }
            _ => {}
        }
        if let Some(known) = known_state(state) {
            self.status = SessionStatus::of(known);
        }
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

    // The most layers that the runtime takes in a frame, the room's layer included.
    pub(crate) fn layer_limit(&self) -> usize {
        self.limit.layers()
    }

    // Whether a frame still has to try how many layers the runtime takes.
    pub(crate) fn layer_trial_due(&self) -> bool {
        self.limit.trial().is_some()
    }

    // The room's layer while the runtime shows the app's layers, which is under all of them.
    fn bottom(&self) -> Option<&xr::CompositionLayerBase<'_, Api>> {
        self.passthrough.layer().filter(|_| self.status.shows())
    }

    // How many layers the room adds to the frame: one or none.
    pub(crate) fn bottom_layers(&self) -> usize {
        usize::from(self.bottom().is_some())
    }

    // Ends the frame begun last. The room is the bottom layer while the runtime shows the app's
    // layers. Then come the layers of `stages`, in the order that the compositor draws them: the
    // last layer covers the others. With `plan`, the panels and rays test against the depth of the
    // projection layer of the app's 3D content. While the runtime has not shown how many layers it
    // takes, `trial` is an empty layer whose copies fill the frame up to the number that the runtime
    // reports. A runtime that refuses that frame gets the same frame again without them.
    pub(crate) fn end_frame<'a>(
        &mut self,
        display_time: xr::Time,
        stages: Stages<
            'a,
            impl Iterator<Item = ProjectionLayer<'a>>,
            impl Iterator<Item = Layer<'a>>,
            impl Iterator<Item = QuadLayer<'a>>,
        >,
        plan: Option<VolumePlan>,
        trial: Option<QuadLayer<'_>>,
    ) -> Result<(), String> {
        self.layers.fill(stages, plan);
        // A layer over the projection layer tests its fragments against the depth that the
        // projection layer carries, and writes none of its own.
        let depth_test = xr::sys::CompositionLayerDepthTestFB {
            ty: xr::sys::CompositionLayerDepthTestFB::TYPE,
            next: ptr::null(),
            depth_mask: xr::sys::FALSE,
            compare_op: xr::sys::CompareOpFB::LESS_OR_EQUAL,
        };
        // The runtime reads the first row of any swapchain as the bottom one. The images of wgpu
        // and the pictures of a surface's producer start with the top row. One layout serves every
        // layer. A layer that tests depth names the layout first and the test after it.
        let flip = xr::sys::CompositionLayerImageLayoutFB {
            ty: xr::sys::CompositionLayerImageLayoutFB::TYPE,
            next: ptr::null_mut(),
            flags: xr::sys::CompositionLayerImageLayoutFlagsFB::VERTICAL_FLIP,
        };
        let flip_then_test = xr::sys::CompositionLayerImageLayoutFB {
            next: ptr::from_ref(&depth_test).cast_mut().cast(),
            ..flip
        };
        // The projection layer that carries the volumes' depth asks for the depth test too, with the
        // write on, because the compositor's depth buffer starts at the far plane and only a layer
        // that writes into it gives the panels over it something to test against
        let write_depth = xr::sys::CompositionLayerDepthTestFB {
            compare_op: xr::sys::CompareOpFB::ALWAYS,
            depth_mask: xr::sys::TRUE,
            ..depth_test
        };
        let testing = plan.is_some_and(|plan| plan.tests_depth);
        let named = |flips: bool, tests: bool| -> *const c_void {
            match (flips, tests) {
                (true, true) => ptr::from_ref(&flip_then_test).cast(),
                (true, false) => ptr::from_ref(&flip).cast(),
                (false, true) => ptr::from_ref(&depth_test).cast(),
                (false, false) => ptr::null(),
            }
        };
        let chains = Chains {
            plain: named(self.flips_layers, false),
            tested: named(self.flips_layers, testing),
            written: if testing {
                ptr::from_ref(&write_depth).cast()
            } else {
                ptr::null()
            },
        };
        let layers = self.layers.len() + self.bottom_layers();
        let counts = self.layers.counts(layers);
        if counts != self.logged_layers {
            self.logged_layers = counts;
            log::info!(
                "cranpose xr: the frame has {} layers, {} of them test depth and {} write it, of {} that the runtime takes",
                counts.layers,
                counts.tested,
                counts.written,
                self.limit.layers(),
            );
        }
        let ended = self.end_trying(display_time, chains, layers, trial);
        self.layers.clear();
        ended
    }

    // Ends the frame of `layers` layers. While the runtime has not shown how many layers it takes,
    // copies of `trial` fill the frame up to the number that it reports. A runtime that refuses
    // that frame gets the same frame again without them.
    fn end_trying(
        &mut self,
        display_time: xr::Time,
        chains: Chains,
        layers: usize,
        trial: Option<QuadLayer<'_>>,
    ) -> Result<(), String> {
        let tried = self
            .limit
            .trial()
            .zip(trial)
            .filter(|(reported, _)| *reported > layers);
        if let Some((reported, empty)) = &tried {
            self.layers.pad(empty, reported - layers);
        }
        let mut ended = self.submit(display_time, chains);
        let mut retried = false;
        if let Some((reported, _)) = tried {
            let taken = ended.is_ok();
            self.limit.tried(taken);
            log::info!(
                "cranpose xr: the runtime takes {} layers of the {reported} that it reports",
                self.limit.layers()
            );
            if !taken {
                self.layers.unpad();
                ended = self.submit(display_time, chains);
                retried = true;
            }
        }
        match ended {
            // A runtime that ends a frame when it refuses it takes no second end of that frame
            Err(xr::sys::Result::ERROR_CALL_ORDER_INVALID) if retried => {
                log::warn!("cranpose xr: the runtime ended the frame that it refused");
                Ok(())
            }
            // The runtime takes fewer layers than it showed. The next frame has fewer layers, and
            // this one is lost.
            Err(xr::sys::Result::ERROR_LAYER_LIMIT_EXCEEDED) if self.limit.refused(layers) => {
                log::warn!(
                    "cranpose xr: the runtime refused {layers} layers and takes {}",
                    self.limit.layers()
                );
                Ok(())
            }
            ended => ended.map_err(|error| format!("OpenXR end frame: {error}")),
        }
    }

    // Hands the layers of the list to the runtime with the room's layer first. The call ends the
    // frame. `chains` are the structs that every flat and curved layer names.
    fn submit(&mut self, display_time: xr::Time, chains: Chains) -> Result<(), xr::sys::Result> {
        let bottom = self.passthrough.layer().filter(|_| self.status.shows());
        let headers = self.layers.headers(bottom, chains);
        // SAFETY: each pointer names the struct of a layer that `self.layers` or
        // `self.passthrough` holds. Nothing moves or drops those until the call returns. The base
        // layer is a transparent wrapper of the header that every layer struct starts with, so a
        // reference to it and a pointer to the header have one layout. The wrapper's `end` hands
        // the array of references to the runtime as an array of pointers.
        let references = unsafe {
            slice::from_raw_parts(
                headers
                    .as_ptr()
                    .cast::<&xr::CompositionLayerBase<'_, Api>>(),
                headers.len(),
            )
        };
        self.stream
            .end(display_time, self.passthrough.blend_mode(), references)
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

// The runtime's state as the backend tells it, or `None` for a state that it does not know.
fn known_state(state: xr::SessionState) -> Option<SessionState> {
    Some(match state {
        xr::SessionState::IDLE => SessionState::Idle,
        xr::SessionState::READY => SessionState::Ready,
        xr::SessionState::SYNCHRONIZED => SessionState::Synchronized,
        xr::SessionState::VISIBLE => SessionState::Visible,
        xr::SessionState::FOCUSED => SessionState::Focused,
        xr::SessionState::STOPPING => SessionState::Stopping,
        xr::SessionState::LOSS_PENDING => SessionState::LossPending,
        xr::SessionState::EXITING => SessionState::Exiting,
        _ => return None,
    })
}

#[cfg(target_os = "android")]
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

pub(crate) fn fovf(fov: &FieldOfView) -> xr::Fovf {
    xr::Fovf {
        angle_left: fov.angle_left,
        angle_right: fov.angle_right,
        angle_up: fov.angle_up,
        angle_down: fov.angle_down,
    }
}

pub(crate) fn field_of_view_of(fov: &xr::Fovf) -> FieldOfView {
    FieldOfView {
        angle_left: fov.angle_left,
        angle_right: fov.angle_right,
        angle_up: fov.angle_up,
        angle_down: fov.angle_down,
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
