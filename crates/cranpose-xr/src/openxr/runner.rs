//! The loop of an immersive app: the host's events, the session's events and the frames.

use std::{cell::RefCell, rc::Rc, time::Duration};

use cranpose::{AppSettings, GpuContext, GuestView, remember};
use cranpose_core::CompositionLocalProvider;
use openxr as xr;

use super::{
    frame_layers::Layer,
    frame_rate::FrameRate,
    graphics::Graphics,
    handle_layers::HandleLayers,
    headset_input::HeadsetInput,
    host::Host,
    panel_layers::{DENSITY, PanelLayers},
    ray_layers::RayLayers,
    runtime::Runtime,
    session::{SessionStatus, XrSession},
    stream_layers::StreamLayers,
    surface_layers::SurfaceLayers,
};
use crate::{
    SpatialCapabilities, SpatialEnvironment,
    depth_order::sort_back_to_front,
    exit_request::{ExitRequest, ExitStep, leave},
    frame_effects::local_frame_effects,
    local_haptics, local_headset_properties, local_spatial_capabilities, local_spatial_environment,
    pointer::Pointer,
    space::{PanelSlot, Space, local_space},
    surface_input::Gesture,
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
    let mut immersive = Immersive::new(runtime, settings, content)?;
    let mut status = SessionStatus::Idle;
    let mut rate = FrameRate::new();
    let mut exit = ExitRequest::default();
    log::info!("cranpose xr: ready");
    while !host.ended(status) {
        status = immersive.xr_session.poll_events()?;
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
                immersive.frame(running)?;
                rate.tick();
            }
        }
    }
    log::info!("cranpose xr: ends in {status:?}");
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
        let xr_session = XrSession::new(runtime, &graphics)?;
        let headset = HeadsetInput::new(&xr_session)?;
        let layers = PanelLayers::new(&xr_session)?;
        let streams = StreamLayers::new(&graphics, &xr_session);
        let format = layers.format();
        let rays = RayLayers::new(&xr_session, &gpu, format)?;
        let handles = HandleLayers::new(&xr_session, &gpu, format)?;
        let capabilities =
            SpatialCapabilities::in_headset(xr_session.passthrough.is_controllable());
        let (effects, haptics, properties) =
            (headset.effects(), headset.haptics(), headset.properties());
        let composed = Rc::new(RefCell::new(None::<(Space, SpatialEnvironment)>));
        let held = Rc::clone(&composed);
        let mut guest = GuestView::new(settings, &gpu, format.wgpu, 16, 16, DENSITY, move || {
            let space = remember(Space::new).with(Clone::clone);
            let environment = remember(SpatialEnvironment::new).with(|environment| *environment);
            held.replace(Some((space.clone(), environment)));
            CompositionLocalProvider(
                [
                    local_space().provides(Some(space)),
                    local_spatial_capabilities().provides(capabilities),
                    local_spatial_environment().provides(Some(environment)),
                    local_frame_effects().provides(Some(effects.clone())),
                    local_haptics().provides(Some(haptics.clone())),
                    local_headset_properties().provides(Some(properties.clone())),
                ],
                &mut content,
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
    // draw and the rays follow the controllers. While the headset shows something over the app
    // the controllers give no input: the pointer sees them lost, which ends a press. While the
    // runtime shows none of the app's layers the app draws none and ends the frame empty. The
    // app's state stays as it was.
    fn frame(&mut self, status: SessionStatus) -> Result<(), String> {
        let frame = self.xr_session.begin_frame()?;
        let aims = self.headset.sample(&mut self.xr_session, &frame, status);
        self.space.track_head(
            self.headset.device_pose(),
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
                std::iter::empty(),
                std::iter::empty(),
                std::iter::empty(),
            )
        }
    }

    // Ends the frame with the layers of the streamed scenes, the panels and the surfaces. The
    // rays go over them. The compositor draws layers in list order, so `panels` go back to front.
    // The flat panels, the curved panels and the surfaces share that order: a panel in front of a
    // surface shows over it. The handle of a movable panel comes right after the panel's own
    // layers, so a panel in front of the handle shows over it.
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
        let shown = panels.iter().flat_map(|slot| {
            layers
                .layer(slot)
                .into_iter()
                .chain(surfaces.layers(slot).map(Layer::Quad))
                .chain(handles.layer(slot, active, head_position).map(Layer::Quad))
        });
        let rays = head_position
            .into_iter()
            .flat_map(|head| self.rays.layers(&beams, head));
        self.xr_session
            .end_frame(display_time, self.streams.layers(), shown, rays)
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
