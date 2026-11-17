//! The loop of an immersive app: the host's events, the session's events and the frames.

use std::{cell::RefCell, rc::Rc, time::Duration};

use cranpose::{AppSettings, GpuContext, GuestView, remember};
use cranpose_core::CompositionLocalProvider;

use super::{
    controllers::Controllers,
    frame_rate::FrameRate,
    graphics::Graphics,
    handle_layers::HandleLayers,
    host::Host,
    panel_layers::{DENSITY, PanelLayers},
    ray_layers::RayLayers,
    runtime::Runtime,
    session::{SessionStatus, XrSession},
};
use crate::{
    pointer::{Pointer, PointerEvent},
    space::{Space, local_space},
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
    log::info!("cranpose xr: ready");
    while !host.ended(status) {
        status = immersive.xr_session.poll_events()?;
        match status {
            SessionStatus::Exit => break,
            SessionStatus::Idle => {}
            SessionStatus::Running => {
                immersive.frame()?;
                rate.tick();
            }
        }
    }
    log::info!("cranpose xr: ends");
    Ok(())
}

// The app in the headset. The fields drop in their order: what draws, then the session, then the
// device and what it draws on.
struct Immersive {
    guest: GuestView,
    space: Space,
    pointer: Pointer,
    layers: PanelLayers,
    rays: RayLayers,
    handles: HandleLayers,
    controllers: Controllers,
    xr_session: XrSession,
    gpu: GpuContext,
    _graphics: Graphics,
}

impl Immersive {
    fn new(
        runtime: Runtime,
        settings: &AppSettings,
        mut content: impl FnMut() + 'static,
    ) -> Result<Self, String> {
        let (graphics, gpu) = Graphics::open(&runtime.instance, runtime.system)?;
        let xr_session = XrSession::new(runtime, &graphics)?;
        let controllers = Controllers::new(&xr_session)?;
        let layers = PanelLayers::new(&xr_session)?;
        let format = layers.format();
        let rays = RayLayers::new(&xr_session, &gpu, format)?;
        let handles = HandleLayers::new(&xr_session, &gpu, format)?;
        let composed = Rc::new(RefCell::new(None::<Space>));
        let held = Rc::clone(&composed);
        let mut guest = GuestView::new(settings, &gpu, format.wgpu, 16, 16, DENSITY, move || {
            let space = remember(Space::new).with(Clone::clone);
            held.replace(Some(space.clone()));
            CompositionLocalProvider([local_space().provides(Some(space))], &mut content);
        })
        .map_err(|error| format!("composition: {error}"))?;
        guest.shell().renderer().set_transparent_background(true);
        // The view composes its content when it is made, and the space stays for the view's life
        let space = composed
            .borrow()
            .clone()
            .ok_or("the content did not compose")?;
        Ok(Self {
            guest,
            space,
            pointer: Pointer::default(),
            layers,
            rays,
            handles,
            controllers,
            xr_session,
            gpu,
            _graphics: graphics,
        })
    }

    // One frame: the controllers move the pointer, the panels that changed draw and the rays
    // follow the controllers.
    fn frame(&mut self) -> Result<(), String> {
        let frame = self.xr_session.begin_frame()?;
        let aims = self
            .controllers
            .aims(&self.xr_session, frame.predicted_display_time);
        let time = Duration::from_nanos(
            frame
                .predicted_display_time
                .as_nanos()
                .max(0)
                .cast_unsigned(),
        );
        let (events, cursor) = self.pointer.update(aims, time, &self.space.panels());
        send(&mut self.guest, events);
        self.space.set_cursor(cursor);
        if let Some((panel, pose)) = self.pointer.moved() {
            self.space.move_panel(panel, pose);
        }
        let mut quads = if frame.should_render {
            self.layers
                .draw(&mut self.guest, &self.gpu, &self.xr_session, &self.space)?
        } else {
            Vec::new()
        };
        // The handles and the rays show over the panels: the rays end on them
        let panels = self.space.panels();
        if frame.should_render {
            quads.extend(self.handles.layers(&panels, self.pointer.handle()));
            if let Some(head) = self.xr_session.head(frame.predicted_display_time) {
                quads.extend(self.rays.layers(&self.pointer.beams(), head.position));
            }
        }
        self.xr_session
            .end_frame(frame.predicted_display_time, &quads)?;
        drop(quads);
        if let Err(error) = self.gpu.device.poll(wgpu::PollType::Poll) {
            log::warn!("cranpose xr: device poll: {error}");
        }
        Ok(())
    }
}

fn send(guest: &mut GuestView, events: &[PointerEvent]) {
    let shell = guest.shell();
    for event in events {
        match *event {
            PointerEvent::Move { x, y } => {
                shell.set_cursor(x, y);
            }
            PointerEvent::Press => {
                shell.pointer_pressed();
            }
            PointerEvent::Release => {
                shell.pointer_released();
            }
            PointerEvent::Leave => {
                shell.set_cursor(AWAY, AWAY);
            }
        }
    }
}
