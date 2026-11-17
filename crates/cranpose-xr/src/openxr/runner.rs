//! The loop of an immersive app: the activity's events, the session's events and the frames.

use std::{cell::RefCell, rc::Rc, time::Duration};

use android_activity::{InputStatus, MainEvent, PollEvent};
use cranpose::{
    AndroidApp, AppLauncher, AppSettings, GpuContext, GuestView, LauncherFonts, remember,
};
use cranpose_core::CompositionLocalProvider;

use super::{
    controllers::Controllers,
    gl_context::GlContext,
    handle_layers::HandleLayers,
    panel_layers::{DENSITY, PanelFrame, PanelLayers},
    ray_layers::RayLayers,
    session::{SessionStatus, XrSession},
    stream_layers::StreamLayers,
    surface_layers::SurfaceLayers,
};
use crate::{
    SpatialCapabilities, SpatialEnvironment, local_spatial_capabilities, local_spatial_environment,
    pointer::{Pointer, PointerEvent},
    space::{Space, local_space},
};

// The most events that one pass of the loop takes from the activity's queue
const MAX_INPUT_EVENTS_PER_LOOP: usize = 16;

// Where the pointer goes when it leaves every panel: far outside the atlas, so no content stays
// hovered.
const AWAY: f32 = -100_000.0;

/// Runs `content` as an immersive app on the headset: its subspaces' panels stand in the room
/// and the controllers point at them. Returns when the headset ends the session or the activity
/// ends.
pub fn run<Fonts: LauncherFonts>(
    launcher: AppLauncher<Fonts>,
    app: AndroidApp,
    content: impl FnMut() + 'static,
) {
    let settings = launcher.start_android_host(&app);
    if let Err(error) = run_session(&app, &settings, content) {
        log::error!("cranpose xr: {error}");
    }
}

fn run_session(
    app: &AndroidApp,
    settings: &AppSettings,
    content: impl FnMut() + 'static,
) -> Result<(), String> {
    let mut immersive = Immersive::new(app, settings, content)?;
    let mut status = SessionStatus::Idle;
    log::info!("cranpose xr: ready");
    while !activity_destroyed(app, status) {
        leave_input_unhandled(app);
        status = immersive.xr_session.poll_events()?;
        match status {
            SessionStatus::Exit => break,
            SessionStatus::Idle => {}
            running => immersive.frame(running)?,
        }
    }
    // The session ends when `immersive` drops, before the context it draws on
    log::info!("cranpose xr: ends in {status:?}");
    Ok(())
}

// Handles the activity's events and returns whether the activity ended. While the headset shows
// something else, it waits for them.
fn activity_destroyed(app: &AndroidApp, status: SessionStatus) -> bool {
    let timeout = if status.runs() {
        Duration::ZERO
    } else {
        Duration::from_millis(100)
    };
    let mut destroyed = false;
    app.poll_events(Some(timeout), |event| {
        if let PollEvent::Main(MainEvent::Destroy) = event {
            destroyed = true;
        }
    });
    destroyed
}

// Takes the activity's queued key and touch events and leaves them unhandled. The controllers
// reach the app through OpenXR. An event that nobody takes from the queue stops the activity from
// answering Android, which shows an "app is not responding" dialog five seconds later.
fn leave_input_unhandled(app: &AndroidApp) {
    let Ok(mut events) = app.input_events_iter() else {
        return;
    };
    for _ in 0..MAX_INPUT_EVENTS_PER_LOOP {
        if !events.next(|_| InputStatus::Unhandled) {
            break;
        }
    }
}

// The app in the headset. The fields drop in their order: what draws, then the session, then the
// device and the context it draws on.
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
    controllers: Controllers,
    xr_session: XrSession,
    gpu: GpuContext,
    _gl: GlContext,
}

impl Immersive {
    fn new(
        app: &AndroidApp,
        settings: &AppSettings,
        mut content: impl FnMut() + 'static,
    ) -> Result<Self, String> {
        let gl = GlContext::new()?;
        let gpu = gl.gpu()?;
        let xr_session = XrSession::new(&gl, app)?;
        let controllers = Controllers::new(&xr_session)?;
        let layers = PanelLayers::new(&xr_session)?;
        let streams = StreamLayers::new(&gl, &xr_session);
        let formats = (layers.gl_format(), layers.format());
        let rays = RayLayers::new(&xr_session, &gpu, formats)?;
        let handles = HandleLayers::new(&xr_session, &gpu, formats)?;
        let capabilities =
            SpatialCapabilities::in_headset(xr_session.passthrough.is_controllable());
        let composed = Rc::new(RefCell::new(None::<(Space, SpatialEnvironment)>));
        let held = Rc::clone(&composed);
        let mut guest = GuestView::new(
            settings,
            &gpu,
            layers.format(),
            16,
            16,
            DENSITY,
            move || {
                let space = remember(Space::new).with(Clone::clone);
                let environment =
                    remember(SpatialEnvironment::new).with(|environment| *environment);
                held.replace(Some((space.clone(), environment)));
                CompositionLocalProvider(
                    [
                        local_space().provides(Some(space)),
                        local_spatial_capabilities().provides(capabilities),
                        local_spatial_environment().provides(Some(environment)),
                    ],
                    &mut content,
                );
            },
        )
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
            controllers,
            xr_session,
            gpu,
            _gl: gl,
        })
    }

    // One frame of a running session. The controllers move the pointer, the panels that changed
    // draw and the rays follow the controllers. While the headset shows something over the app
    // the controllers give no input: the pointer sees them lost, which ends a press. While the
    // runtime shows none of the app's layers the app draws none and ends the frame empty. The
    // app's state stays as it was.
    fn frame(&mut self, status: SessionStatus) -> Result<(), String> {
        let frame = self.xr_session.begin_frame()?;
        let aims = if status.listens() {
            self.controllers
                .aims(&self.xr_session, frame.predicted_display_time)
        } else {
            [None, None]
        };
        let shown = status.shows() && frame.should_render;
        let time = Duration::from_nanos(frame.predicted_display_time.as_nanos().max(0) as u64);
        let (events, cursor) = self.pointer.update(aims, time, &self.space.panels());
        send(&mut self.guest, events);
        self.space.set_cursor(cursor);
        if let Some((panel, pose)) = self.pointer.moved() {
            self.space.move_panel(panel, pose);
        }
        self.follow_environment();
        let PanelFrame {
            mut quads,
            cylinders,
        } = if shown {
            self.layers
                .draw(&mut self.guest, &self.gpu, &self.xr_session, &self.space)?
        } else {
            PanelFrame {
                quads: Vec::new(),
                cylinders: Vec::new(),
            }
        };
        // The handles and the rays show over the panels: the rays end on them
        let panels = self.space.panels();
        if shown {
            self.streams.update(&self.xr_session, &self.space);
        } else {
            self.streams.skip(&self.space);
        }
        self.surfaces.sync(&self.xr_session, &self.space, &panels);
        if shown {
            quads.extend(self.surfaces.layers(&panels));
            quads.extend(self.handles.layers(&panels, self.pointer.handle()));
            if let Some(head) = self.xr_session.head(frame.predicted_display_time) {
                quads.extend(self.rays.layers(&self.pointer.beams(), head.position));
            }
        }
        let streams = self.streams.layers();
        self.xr_session
            .end_frame(frame.predicted_display_time, &streams, &cylinders, &quads)?;
        drop((streams, cylinders, quads));
        self.gpu.device.poll(wgpu::PollType::Poll).ok();
        Ok(())
    }

    // The room follows what the app prefers, and the app learns what the room shows.
    fn follow_environment(&mut self) {
        let passthrough = &mut self.xr_session.passthrough;
        passthrough.set_opacity(self.environment.preference().unwrap_or(0.0));
        self.environment.show(passthrough.opacity());
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
