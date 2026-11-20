//! The loop of an immersive app: the host's events, the session's events and the frames.

use std::{cell::RefCell, rc::Rc, time::Duration};

use cranpose::{
    AppSettings, GpuContext, GuestView, LifecycleState, dispatch_lifecycle_state, remember,
};
use cranpose_core::CompositionLocalProvider;
use openxr as xr;

use super::{
    frame_layers::{Layer, Stages},
    frame_rate::FrameRate,
    graphics::Graphics,
    handle_layers::HandleLayers,
    headset_input::HeadsetInput,
    host::Host,
    panel_layers::{DENSITY, PanelLayers},
    ray_layers::RayLayers,
    runtime::Runtime,
    session::{ProjectionLayer, QuadLayer, XrSession},
    stream_layers::StreamLayers,
    surface_layers::SurfaceLayers,
    volume_layers::VolumeLayers,
};
use crate::{
    SpatialCapabilities, SpatialEnvironment,
    depth_order::sort_back_to_front,
    exit_request::{ExitRequest, ExitStep, leave},
    frame_effects::local_frame_effects,
    local_haptics, local_headset_properties, local_spatial_capabilities, local_spatial_environment,
    local_tracking_capabilities,
    pointer::Pointer,
    session_status::{SessionStatus, StatusWatch},
    space::{PanelSlot, Ride, Space, local_space},
    surface_input::Gesture,
    volume_host::VolumeScene,
};

// Where the pointer goes when it leaves every panel: far outside the atlas, so no content stays
// hovered.
const AWAY: f32 = -100_000.0;

// Runs `content` on the headset of `runtime` until the headset or the host ends it.
pub(crate) fn run_immersive(
    host: &Host,
    runtime: Runtime,
    settings: &AppSettings,
    content: impl FnMut() + 'static,
) -> Result<(), String> {
    // A run that follows another in the same process starts as the first did
    dispatch_lifecycle_state(LifecycleState::Created);
    let mut immersive = Immersive::new(runtime, settings, content)?;
    let mut status = SessionStatus::Idle;
    let mut rate = FrameRate::new();
    let mut watch = StatusWatch::default();
    let mut exit = ExitRequest::default();
    log::info!("cranpose xr: ready");
    while !host.ended(status) {
        status = immersive.xr_session.poll_events()?;
        // The app learns of a change as Compose's lifecycle, before the frame that it changes
        if let Some(change) = watch.change(status) {
            log::info!(
                "cranpose xr: {:?} ran {} frames, now {:?}, lifecycle {:?}",
                change.from,
                change.frames,
                change.to,
                change.lifecycle
            );
            dispatch_lifecycle_state(change.lifecycle);
        }
        match exit.poll(status.runs()) {
            ExitStep::Wait => {}
            ExitStep::EndSession => {
                if let Err(error) = immersive.xr_session.request_exit() {
                    log::warn!("cranpose xr: {error}");
                    break;
                }
            }
            ExitStep::Leave => break,
        }
        match status {
            SessionStatus::Exit => break,
            SessionStatus::Idle => {}
            running => {
                immersive.frame(running, host)?;
                rate.tick();
                watch.ran();
            }
        }
    }
    log::info!("cranpose xr: ends in {status:?}, lifecycle Destroyed");
    dispatch_lifecycle_state(LifecycleState::Destroyed);
    // The session ends when `immersive` drops, before the device it draws on. The host closes
    // after that, when the app asked for it.
    drop(immersive);
    leave(|| host.finish());
    Ok(())
}

// The app in the headset. The fields drop in their order: what draws, then the session, then the
// device and what it draws on.
struct Immersive {
    guest: GuestView,
    space: Space,
    environment: SpatialEnvironment,
    pointer: Pointer,
    layers: PanelLayers,
    volumes: VolumeLayers,
    streams: StreamLayers,
    surfaces: SurfaceLayers,
    rays: RayLayers,
    handles: HandleLayers,
    headset: HeadsetInput,
    xr_session: XrSession,
    gpu: GpuContext,
    _graphics: Graphics,
    // The panels of the frame, read anew at each frame into the same storage
    panels: Vec<PanelSlot>,
}

impl Immersive {
    fn new(
        runtime: Runtime,
        settings: &AppSettings,
        mut content: impl FnMut() + 'static,
    ) -> Result<Self, String> {
        let (graphics, gpu) = Graphics::open(&runtime.instance, runtime.system)?;
        let tracking = runtime.tracking;
        let xr_session = XrSession::new(runtime, &graphics)?;
        let headset = HeadsetInput::new(&xr_session, tracking)?;
        let layers = PanelLayers::new(&xr_session)?;
        let streams = StreamLayers::new(&graphics, &xr_session);
        let format = layers.format();
        let volumes = VolumeLayers::new(&xr_session, format);
        let (width, height) = headset.eye_image_size();
        let scene_size = [width as f32, height as f32];
        let rays = RayLayers::new(&xr_session, &gpu, format)?;
        let handles = HandleLayers::new(&xr_session, &gpu, format)?;
        let capabilities =
            SpatialCapabilities::in_headset(xr_session.passthrough.is_controllable());
        let (effects, haptics, properties) =
            (headset.effects(), headset.haptics(), headset.properties());
        let composed = Rc::new(RefCell::new(None::<(Space, SpatialEnvironment)>));
        let held = Rc::clone(&composed);
        let mut guest = GuestView::new(settings, &gpu, format.wgpu, 16, 16, DENSITY, move || {
            let space = remember(move || Space::with_scene_size(scene_size)).with(Clone::clone);
            let environment = remember(SpatialEnvironment::new).with(|environment| *environment);
            held.replace(Some((space.clone(), environment)));
            let volume_space = space.clone();
            CompositionLocalProvider(
                [
                    local_space().provides(Some(space)),
                    local_spatial_capabilities().provides(capabilities),
                    local_spatial_environment().provides(Some(environment)),
                    local_frame_effects().provides(Some(effects.clone())),
                    local_haptics().provides(Some(haptics.clone())),
                    local_headset_properties().provides(Some(properties.clone())),
                    local_tracking_capabilities().provides(tracking.capabilities),
                ],
                || {
                    content();
                    VolumeScene(volume_space.clone());
                },
            );
        })
        .map_err(|error| format!("composition: {error}"))?;
        guest.shell().renderer().set_transparent_background(true);
        // The view composes its content when it is made, and the space stays for the view's life
        let (space, environment) = composed
            .borrow()
            .clone()
            .ok_or("the content did not compose")?;
        space.set_curves_shown(xr_session.shows_cylinders());
        Ok(Self {
            guest,
            space,
            environment,
            pointer: Pointer::default(),
            layers,
            volumes,
            streams,
            surfaces: SurfaceLayers::new(),
            rays,
            handles,
            headset,
            xr_session,
            gpu,
            _graphics: graphics,
            panels: Vec::new(),
        })
    }

    // One frame of a running session. The controllers move the pointer, the panels that changed
    // draw and the rays follow the controllers. While the app has no input focus the pointer
    // gets no aim. It sees the controllers lost, which ends a press. No ray shows. The panels
    // and the volumes keep drawing, as VRC.Quest.Input.4 requires. While the runtime shows none
    // of the app's layers the app draws none and ends the frame empty. The app's state stays as
    // it was.
    fn frame(&mut self, status: SessionStatus, host: &Host) -> Result<(), String> {
        let frame = self.xr_session.begin_frame()?;
        let aims = status.input(
            self.headset
                .sample(&mut self.xr_session, &frame, status, host),
        );
        self.space.track(
            self.headset.device_pose(),
            self.headset.controllers(),
            self.headset.display_time(),
            self.headset.recenter(),
        );
        let shown = status.shows() && frame.should_render;
        let time = Duration::from_nanos(
            frame
                .predicted_display_time
                .as_nanos()
                .max(0)
                .cast_unsigned(),
        );
        self.space.panels_into(&mut self.panels);
        let (events, cursor) = self.pointer.update(aims, time, &self.panels);
        send(&mut self.guest, events);
        for gesture in self.pointer.surface_events() {
            self.space.deliver_to_surface(gesture);
        }
        self.space.set_cursor(cursor);
        for step in self.pointer.moves() {
            self.space.carry(step);
        }
        for step in self.pointer.resizes() {
            self.space.resize(step);
        }
        self.follow_environment();
        let mut panels = std::mem::take(&mut self.panels);
        let presented = self.present(frame.predicted_display_time, shown, &mut panels);
        self.panels = panels;
        presented?;
        if let Err(error) = self.gpu.device.poll(wgpu::PollType::Poll) {
            log::warn!("cranpose xr: device poll: {error}");
        }
        Ok(())
    }

    // Draws the panels that changed and ends the frame with the layers. `panels` takes the
    // panels after the draw: the draw composes, and their parts are where it drew them.
    fn present(
        &mut self,
        display_time: xr::Time,
        shown: bool,
        panels: &mut Vec<PanelSlot>,
    ) -> Result<(), String> {
        if shown {
            self.layers.draw(
                &mut self.guest,
                &self.gpu,
                &self.xr_session,
                &self.space,
                panels,
            )?;
            self.streams
                .update(&self.xr_session, &self.space, self.headset.display_time());
            self.volumes.draw(
                &mut self.guest,
                &self.gpu,
                &self.xr_session,
                &self.space,
                self.headset.eyes(),
                self.headset.eye_image_size(),
            );
        } else {
            self.space.panels_into(panels);
            self.streams.skip(&self.space);
        }
        self.surfaces.sync(&self.xr_session, &self.space, panels);
        if shown {
            self.end_with_layers(display_time, panels)
        } else {
            self.xr_session.end_frame(
                display_time,
                Stages {
                    streams: std::iter::empty::<ProjectionLayer<'_>>(),
                    volumes: None,
                    panels: std::iter::empty::<Layer<'_>>(),
                    overlay: std::iter::empty::<QuadLayer<'_>>(),
                },
                None,
            )
        }
    }

    // Ends the frame with the layers of the streamed scenes, the panels and the surfaces. The
    // rays go over them. The compositor draws layers in list order, so `panels` go back to front.
    // The flat panels, the curved panels and the surfaces share that order: a panel in front of a
    // surface shows over it. The handles of a panel come right after the panel's own
    // layers, so a panel in front of a handle shows over it.
    fn end_with_layers(
        &mut self,
        display_time: xr::Time,
        panels: &mut [PanelSlot],
    ) -> Result<(), String> {
        let head = self.headset.device_pose();
        if let Some(head) = &head {
            sort_back_to_front(panels, head.position, |slot| &slot.pose);
        }
        let beams = self.pointer.beams();
        let active = self.pointer.handle();
        let head_position = head.as_ref().map(|head| head.position);
        let (layers, surfaces, handles) = (&self.layers, &self.surfaces, &self.handles);
        let shown = panels
            .iter()
            .filter(|slot| slot.ride != Ride::Hidden)
            .flat_map(|slot| {
                layers
                    .layer(slot)
                    .into_iter()
                    .chain(surfaces.layers(slot).map(Layer::Quad))
                    .chain(handles.layers(slot, active, head_position).map(Layer::Quad))
            });
        let rays = head_position
            .into_iter()
            .flat_map(|head| self.rays.layers(&beams, head));
        self.xr_session.end_frame(
            display_time,
            Stages {
                streams: self.streams.layers(),
                volumes: self.volumes.layer(),
                panels: shown,
                overlay: rays,
            },
            self.volumes.plan(),
        )
    }

    // The room follows what the app prefers, and the app learns what the room shows.
    fn follow_environment(&mut self) {
        let passthrough = &mut self.xr_session.passthrough;
        passthrough.set_opacity(self.environment.preference().unwrap_or(0.0));
        self.environment.show(passthrough.opacity());
    }
}

fn send(guest: &mut GuestView, events: &[Gesture]) {
    let shell = guest.shell();
    for event in events {
        match *event {
            Gesture::Move { x, y } => {
                shell.set_cursor(x, y);
            }
            Gesture::Press => {
                shell.pointer_pressed();
            }
            Gesture::Release => {
                shell.pointer_released();
            }
            Gesture::Leave => {
                shell.set_cursor(AWAY, AWAY);
            }
        }
    }
}
