//! The OpenXR session, its events and the ends of the frames.
#![expect(
    unsafe_code,
    reason = "layers made from raw structs with extension structs in their chain"
)]

use std::ptr;

use openxr as xr;

use super::{
    graphics::{Api, Graphics},
    runtime::Runtime,
};
use crate::Pose;

const VIEW_CONFIGURATION: xr::ViewConfigurationType = xr::ViewConfigurationType::PRIMARY_STEREO;

// Where the session stands after its events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SessionStatus {
    // The headset shows something else: the app waits.
    Idle,
    // The app draws frames.
    Running,
    // The runtime ends the session: the app ends.
    Exit,
}

// A layer that shows a whole swapchain image on a flat panel.
pub(crate) struct QuadLayer<'a> {
    pub(crate) swapchain: &'a xr::Swapchain<Api>,
    pub(crate) image_size: (u32, u32),
    pub(crate) pose: Pose,
    // The panel's width and height in meters.
    pub(crate) size: [f32; 2],
}

pub(crate) struct XrSession {
    instance: xr::Instance,
    pub(crate) session: xr::Session<Api>,
    waiter: xr::FrameWaiter,
    stream: xr::FrameStream<Api>,
    // The room as the app sees it: its origin is where the head was when the app started
    pub(crate) local: xr::Space,
    // The head's own space, which the runtime locates for each frame
    view: xr::Space,
    blend: xr::EnvironmentBlendMode,
    // Whether layers say that their images start with the top row, because the API's own first
    // row is the bottom one and the runtime can flip it
    flips_layers: bool,
    running: bool,
}

impl XrSession {
    // Makes a session on the device of `graphics`, which must outlive the session.
    pub(crate) fn new(runtime: Runtime, graphics: &Graphics) -> Result<Self, String> {
        let Runtime {
            instance,
            system,
            flips_layers,
        } = runtime;
        if !Graphics::TOP_ROW_FIRST && !flips_layers {
            log::warn!("cranpose xr: the runtime cannot flip layers; panels show upside down");
        }
        let (session, waiter, stream) = graphics.create_session(&instance, system)?;
        let local = session
            .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            .map_err(|error| format!("OpenXR space: {error}"))?;
        let view = session
            .create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)
            .map_err(|error| format!("OpenXR view space: {error}"))?;
        let blend = instance
            .enumerate_environment_blend_modes(system, VIEW_CONFIGURATION)
            .ok()
            .and_then(|modes| modes.first().copied())
            .unwrap_or(xr::EnvironmentBlendMode::OPAQUE);
        Ok(Self {
            instance,
            session,
            waiter,
            stream,
            local,
            view,
            blend,
            flips_layers: flips_layers && !Graphics::TOP_ROW_FIRST,
            running: false,
        })
    }

    pub(crate) fn instance(&self) -> &xr::Instance {
        &self.instance
    }

    // Whether layers show their images from the top row, which is how wgpu writes them. If not,
    // the first row is the bottom one.
    pub(crate) fn top_row_first(&self) -> bool {
        Graphics::TOP_ROW_FIRST || self.flips_layers
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

    // Handles the runtime's events: starts and stops the session as the headset asks.
    pub(crate) fn poll_events(&mut self) -> Result<SessionStatus, String> {
        let mut buffer = xr::EventDataBuffer::new();
        while let Some(event) = self
            .instance
            .poll_event(&mut buffer)
            .map_err(|error| format!("OpenXR events: {error}"))?
        {
            match event {
                xr::Event::SessionStateChanged(change) => {
                    log::info!("cranpose xr: session {:?}", change.state());
                    match change.state() {
                        xr::SessionState::READY => {
                            self.session
                                .begin(VIEW_CONFIGURATION)
                                .map_err(|error| format!("OpenXR begin: {error}"))?;
                            self.running = true;
                        }
                        xr::SessionState::STOPPING => {
                            self.session
                                .end()
                                .map_err(|error| format!("OpenXR end: {error}"))?;
                            self.running = false;
                        }
                        xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                            return Ok(SessionStatus::Exit);
                        }
                        _ => {}
                    }
                }
                xr::Event::InstanceLossPending(_) => return Ok(SessionStatus::Exit),
                _ => {}
            }
        }
        Ok(if self.running {
            SessionStatus::Running
        } else {
            SessionStatus::Idle
        })
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

    // Ends the frame begun last with `layers`, back to front.
    pub(crate) fn end_frame(
        &mut self,
        display_time: xr::Time,
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
        let quads: Vec<xr::CompositionLayerQuad<'_, Api>> = layers
            .iter()
            .zip(layouts.iter_mut())
            .map(|(layer, layout)| {
                let raw = xr::sys::CompositionLayerQuad {
                    ty: xr::sys::CompositionLayerQuad::TYPE,
                    next: if self.flips_layers {
                        ptr::from_mut(layout).cast_const().cast()
                    } else {
                        ptr::null()
                    },
                    layer_flags: xr::CompositionLayerFlags::BLEND_TEXTURE_SOURCE_ALPHA,
                    space: self.local.as_raw(),
                    eye_visibility: xr::EyeVisibility::BOTH,
                    sub_image: xr::sys::SwapchainSubImage {
                        swapchain: layer.swapchain.as_raw(),
                        image_rect: xr::Rect2Di {
                            offset: xr::Offset2Di { x: 0, y: 0 },
                            extent: xr::Extent2Di {
                                width: layer.image_size.0 as i32,
                                height: layer.image_size.1 as i32,
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
        let references: Vec<&xr::CompositionLayerBase<'_, Api>> =
            quads.iter().map(|quad| &**quad).collect();
        self.stream
            .end(display_time, self.blend, &references)
            .map_err(|error| format!("OpenXR end frame: {error}"))
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
