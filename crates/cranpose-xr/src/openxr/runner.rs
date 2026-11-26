//! The loop of an immersive app: the host's events, the session's events and the frames.

use std::{cell::RefCell, rc::Rc, time::Duration};

use cranpose::{
    AppSettings, GpuContext, GuestView, LifecycleState, default_audio, dispatch_lifecycle_state,
    remember,
};
use cranpose_core::CompositionLocalProvider;
use openxr as xr;

use super::{
    entitlement_run::EntitlementRun,
    frame_layers::{Layer, Stages},
    frame_rate::FrameRate,
    graphics::Graphics,
    handle_layers::{self, Handle, HandleLayers},
    headset_input::HeadsetInput,
    host::{Host, SystemKeyboard},
    panel_layers::{DENSITY, PanelLayers},
    ray_layers::RayLayers,
    render_models::RenderModels,
    runtime::Runtime,
    session::{ProjectionLayer, QuadLayer, XrSession},
    spatial::{Frame as RoomFrame, Room, SpatialEvent},
    stream_layers::StreamLayers,
    surface_layers::SurfaceLayers,
    trial_layer::TrialLayer,
    volume_layers::{ProjectionFrame, VolumeLayers},
};
use crate::{
    Anchors, EntitlementCheck, Hand, Planes, SpatialCapabilities, SpatialEnvironment, atlas_input,
    depth_order::{plane_distance, sort_back_to_front},
    exit_request::{ExitRequest, ExitStep, leave},
    frame_effects::local_frame_effects,
    keyboard_panel::SoftKeyboardPanel,
    layer_budget::{FixedLayers, LayerBudget, PanelNeed, Place, Projection},
    local_anchors, local_controller_models, local_entitlement_check, local_haptics,
    local_headset_properties, local_planes, local_pointer_settings, local_spatial_capabilities,
    local_spatial_environment, local_tracking_capabilities,
    pointers::Pointers,
    projected_pass::ProjectedLayer,
    session_status::{SessionStatus, StatusWatch},
    soft_keyboard::SoftKeyboard,
    sound_stage::SoundStage,
    space::{PanelSlot, Ride, Space, local_space},
    spatial_store::SpatialStore,
    volume_host::VolumeScene,
    volume_pick::PickMeshes,
    volumes::scene_count,
};

// Which pictures of a frame keep a layer of their own, and the pictures that the projection layer
// draws instead, back to front. Both keep their storage from frame to frame.
#[derive(Default)]
struct FramePlan {
    budget: LayerBudget,
    projected: Vec<ProjectedLayer>,
}

// Runs `content` on the headset of `runtime` until the headset or the host ends it.
pub(crate) fn run_immersive(
    host: &Host,
    runtime: Runtime,
    settings: &AppSettings,
    mut entitlement: Option<EntitlementRun>,
    content: impl FnMut() + 'static,
) -> Result<(), String> {
    // A run that follows another in the same process starts as the first did
    dispatch_lifecycle_state(LifecycleState::Created);
    let mut immersive = Immersive::new(host, runtime, settings, entitlement.is_some(), content)?;
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
            immersive.set_lifecycle(change.lifecycle);
        }
        if let (Some(run), Some(check)) = (&mut entitlement, &immersive.entitlement) {
            run.poll(host, check);
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
    immersive.set_lifecycle(LifecycleState::Destroyed);
    // The session ends when `immersive` drops, before the device it draws on. The host closes
    // after that, when the app asked for it.
    drop(immersive);
    leave(|| host.finish());
    Ok(())
}

// What the first composition makes and the backend keeps: the space, the environment, the
// entitlement check when one runs and the room's store.
type Composed = (
    Space,
    SpatialEnvironment,
    Option<EntitlementCheck>,
    Rc<SpatialStore>,
);

// The app in the headset. The fields drop in their order: what draws, then the session, then the
// device and what it draws on.
struct Immersive {
    guest: GuestView,
    space: Space,
    // The keyboard that a focused text field opens: the system's where the runtime shows it, else
    // the one that Cranpose draws
    keyboard: SoftKeyboard,
    system_keyboard: Option<SystemKeyboard>,
    environment: SpatialEnvironment,
    // The app's handle on the entitlement check, when one runs
    entitlement: Option<EntitlementCheck>,
    // The room's anchors and planes: what the runtime holds, and what the app reads of it
    room: Room,
    store: Rc<SpatialStore>,
    // The events about spatial entities that the last frame read, which keep their storage
    room_events: Vec<SpatialEvent>,
    pointers: Pointers,
    // The meshes of the volumes that a ray points at, for the pointer to meet
    picks: PickMeshes,
    layers: PanelLayers,
    volumes: VolumeLayers,
    streams: StreamLayers,
    surfaces: SurfaceLayers,
    rays: RayLayers,
    handles: HandleLayers,
    // What the runtime offers for the controllers' models, and which questions it has answered
    render_models: RenderModels,
    // The sound of the app, or `None` while its audio player has no spatial sound
    sounds: Option<SoundStage>,
    headset: HeadsetInput,
    xr_session: XrSession,
    gpu: GpuContext,
    _graphics: Graphics,
    // The panels of the frame, read anew at each frame into the same storage
    panels: Vec<PanelSlot>,
    plan: FramePlan,
    // The empty layer that the first shown frame repeats, until that frame has tried the limit
    trial: Option<TrialLayer>,
}

impl Immersive {
    fn new(
        host: &Host,
        runtime: Runtime,
        settings: &AppSettings,
        entitled: bool,
        mut content: impl FnMut() + 'static,
    ) -> Result<Self, String> {
        let (graphics, gpu) = Graphics::open(&runtime.instance, runtime.system)?;
        let tracking = runtime.tracking;
        let spatial = runtime.spatial;
        let render_models = RenderModels::new(runtime.render_models);
        let xr_session = XrSession::new(runtime, &graphics)?;
        let room = Room::new(spatial, &xr_session);
        let headset = HeadsetInput::new(&xr_session, tracking)?;
        let layers = PanelLayers::new(&xr_session)?;
        let streams = StreamLayers::new(&graphics, &xr_session);
        let format = layers.format();
        let volumes = VolumeLayers::new(&xr_session, format);
        let (width, height) = headset.eye_image_size();
        let scene_size = [width as f32, height as f32];
        let rays = RayLayers::new(&xr_session, &gpu, format)?;
        let handles = HandleLayers::new(&xr_session, &gpu, format)?;
        let trial = TrialLayer::new(&xr_session, &gpu, format)?;
        let sounds = SoundStage::new(default_audio());
        let capabilities =
            SpatialCapabilities::in_headset(xr_session.passthrough.is_controllable())
                .with_spatial_audio(sounds.is_some());
        let shared = headset.handles();
        let composed = Rc::new(RefCell::new(None::<Composed>));
        let held = Rc::clone(&composed);
        let mut guest = GuestView::new(settings, &gpu, format.wgpu, 16, 16, DENSITY, move || {
            let space = remember(move || Space::with_scene_size(scene_size)).with(Clone::clone);
            let environment = remember(SpatialEnvironment::new).with(|environment| *environment);
            // The handle lives in the composition, as the state of a composition does
            let check = entitled.then(|| remember(EntitlementCheck::new).with(|check| *check));
            // The room's anchors and planes belong to the composition, as its states do
            let store = remember(move || SpatialStore::new(spatial.anchors, spatial.planes))
                .with(Clone::clone);
            space.attach_spatial_store(&store);
            held.replace(Some((space.clone(), environment, check, Rc::clone(&store))));
            let volume_space = space.clone();
            CompositionLocalProvider(
                [
                    local_space().provides(Some(space)),
                    local_spatial_capabilities().provides(capabilities),
                    local_spatial_environment().provides(Some(environment)),
                    local_frame_effects().provides(Some(shared.effects.clone())),
                    local_haptics().provides(Some(shared.haptics.clone())),
                    local_pointer_settings().provides(Some(shared.pointer_settings.clone())),
                    local_controller_models()
                        .provides(Some(volume_space.volumes().controllers().handle())),
                    local_headset_properties().provides(Some(shared.properties.clone())),
                    local_tracking_capabilities().provides(tracking.capabilities),
                    local_entitlement_check().provides(check),
                    local_anchors().provides(Some(Anchors::new(Rc::clone(&store)))),
                    local_planes().provides(Some(Planes::new(store))),
                ],
                || {
                    content();
                    VolumeScene(volume_space.clone());
                    SoftKeyboardPanel(volume_space.clone());
                },
            );
        })
        .map_err(|error| format!("composition: {error}"))?;
        guest.shell().renderer().set_transparent_background(true);
        // The view composes its content when it is made, and the space stays for the view's life
        let (space, environment, entitlement, store) = composed
            .borrow()
            .clone()
            .ok_or("the content did not compose")?;
        space.set_curves_shown(xr_session.shows_cylinders());
        render_models.announce(space.volumes().controllers());
        let (keyboard, system_keyboard) = install_keyboard(host, &space, &mut guest);
        Ok(Self {
            guest,
            space,
            keyboard,
            system_keyboard,
            environment,
            entitlement,
            room,
            store,
            room_events: Vec::new(),
            pointers: Pointers::default(),
            picks: PickMeshes::default(),
            layers,
            volumes,
            streams,
            surfaces: SurfaceLayers::new(),
            rays,
            handles,
            render_models,
            sounds,
            headset,
            xr_session,
            gpu,
            _graphics: graphics,
            panels: Vec::new(),
            plan: FramePlan::default(),
            trial,
        })
    }

    // Tells the app's sound what the lifecycle is, which silences it while the app has no input
    // focus.
    fn set_lifecycle(&self, state: LifecycleState) {
        if let Some(sounds) = &self.sounds {
            sounds.lifecycle(state);
        }
    }

    // One frame of a running session. The controllers move the pointer, the panels that changed
    // draw and the rays follow the controllers. While the app has no input focus the pointer
    // gets no aim. It sees the controllers lost, which ends a press. No ray shows. The panels
    // and the volumes keep drawing, as VRC.Quest.Input.4 requires. While the runtime shows none
    // of the app's layers the app draws none and ends the frame empty. The app's state stays as
    // it was.
    fn frame(&mut self, status: SessionStatus, host: &Host) -> Result<(), String> {
        self.ask_for_models();
        let frame = self.xr_session.begin_frame()?;
        let aims = status.input(
            self.headset
                .sample(&mut self.xr_session, &frame, status, host),
        );
        self.xr_session
            .spatial_events
            .take_into(&mut self.room_events);
        self.room.sample(&RoomFrame {
            xr_session: &self.xr_session,
            host,
            store: &self.store,
            events: &self.room_events,
            now: self.headset.now(),
            time: frame.predicted_display_time,
            head_tracked: self.headset.device_pose().is_some(),
        });
        self.room_events.clear();
        self.space.track(
            &self.headset.follow_poses(),
            self.headset.display_time(),
            self.headset.recenter(),
        );
        self.show_controllers();
        let shown = status.shows() && frame.should_render;
        let time = Duration::from_nanos(
            frame
                .predicted_display_time
                .as_nanos()
                .max(0)
                .cast_unsigned(),
        );
        self.space.panels_into(&mut self.panels);
        if let Some(root) = self.volumes.scene_root() {
            let pressed = self.pointers.pressed_surfaces();
            let pointed = |slot: &PanelSlot| {
                pressed.contains(&Some(slot.id))
                    || aims
                        .iter()
                        .flatten()
                        .any(|aim| slot.hit(&aim.ray).is_some())
            };
            self.picks
                .refresh((&mut self.guest, root), &self.space, &self.panels, pointed);
        }
        let aims = self
            .space
            .volumes()
            .controllers()
            .start_rays(aims, Hand::BOTH.map(|hand| self.headset.controller(hand)));
        #[cfg(all(feature = "scripted-hand", feature = "gltf"))]
        super::ray_start_log::log(
            self.headset.now(),
            aims,
            self.space.volumes().controllers(),
            Hand::BOTH.map(|hand| self.headset.controller(hand).aim),
        );
        self.pointers.update(
            aims,
            time,
            (&self.panels, &self.picks),
            self.headset.pointer_choice(),
        );
        for (hand, tick) in Hand::BOTH.into_iter().zip(self.pointers.ticks()) {
            if tick {
                self.headset.tick(hand);
            }
        }
        atlas_input::deliver(self.guest.shell(), self.pointers.atlas());
        match &self.system_keyboard {
            Some(system) => system.pump(&mut self.guest),
            None => self.keyboard.pump(self.guest.shell(), &self.space),
        }
        for gesture in self.pointers.surface_events() {
            self.space.deliver_to_surface(gesture);
        }
        for (hand, cursor) in Hand::BOTH.into_iter().zip(self.pointers.cursors()) {
            self.space.set_cursor(hand, cursor);
        }
        for step in self.pointers.moves() {
            self.space.carry(step);
        }
        for step in self.pointers.resizes() {
            self.space.resize(step);
        }
        self.follow_environment();
        if let Some(sounds) = &mut self.sounds {
            sounds.update(self.headset.device_pose(), &self.space);
        }
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
        if !self.xr_session.layer_trial_due() {
            self.trial = None;
        }
        if !shown {
            self.space.panels_into(panels);
            self.streams.skip(&self.space);
            self.surfaces.sync(&self.xr_session, &self.space, panels);
            return self.xr_session.end_frame(
                display_time,
                Stages {
                    streams: std::iter::empty::<ProjectionLayer<'_>>(),
                    volumes: None,
                    panels: std::iter::empty::<Layer<'_>>(),
                    overlay: std::iter::empty::<QuadLayer<'_>>(),
                },
                None,
                None,
            );
        }
        self.layers.draw(
            &mut self.guest,
            &self.gpu,
            &self.xr_session,
            &self.space,
            panels,
        )?;
        self.streams
            .update(&self.xr_session, &self.space, self.headset.display_time());
        self.surfaces.sync(&self.xr_session, &self.space, panels);
        // The compositor draws layers in list order, so the panels go back to front
        let head = self.headset.device_pose().map(|head| head.position);
        if let Some(head) = head {
            sort_back_to_front(panels, head, |slot| &slot.pose);
        }
        self.plan_layers(panels, head);
        self.volumes.draw(
            &mut self.guest,
            &self.gpu,
            &self.xr_session,
            &self.space,
            &ProjectionFrame {
                views: self.headset.eyes(),
                size: self.headset.eye_image_size(),
                layers: &self.plan.projected,
                atlas: self.layers.atlas(),
            },
        );
        self.end_with_layers(display_time, panels, head)
    }

    // Decides which pictures of the frame keep a layer of their own in the runtime's limit, and
    // lists the pictures that the projection layer draws instead, back to front as `panels` stand.
    // A frame within the limit lists none and draws nothing more.
    fn plan_layers(&mut self, panels: &[PanelSlot], head: Option<[f32; 3]>) {
        let active = self.pointers.handles();
        let cursors = self.pointers.cursors();
        let pressed = self.pointers.pressed_surfaces();
        let pointed = |id: u64| {
            cursors.iter().flatten().any(|cursor| cursor.panel == id)
                || active.iter().flatten().any(|focus| focus.panel == id)
                || pressed.contains(&Some(id))
        };
        self.plan.budget.clear();
        for slot in panels {
            self.plan.budget.push(if slot.ride == Ride::Hidden {
                PanelNeed::default()
            } else {
                PanelNeed {
                    id: slot.id,
                    distance: head.map_or(0.0, |head| plane_distance(head, &slot.pose)),
                    priority: slot.priority,
                    pointed: pointed(slot.id),
                    content: self.layers.layer(slot).is_some(),
                    surfaces: self.surfaces.layers(slot).count(),
                    handles: handle_layers::placed(slot, active, head).count(),
                }
            });
        }
        let beams = self.pointers.beams();
        let rays = head.map_or(0, |head| self.rays.layers(&beams, head).count());
        let projection = if !self.volumes.available() {
            Projection::Unavailable
        } else if scene_count(&self.space) > 0 {
            Projection::Present
        } else {
            Projection::Absent
        };
        let fixed = FixedLayers {
            layers: self.xr_session.bottom_layers() + self.streams.layers().count() + rays,
            projection,
        };
        let budget = self.plan.budget.plan(self.xr_session.layer_limit(), fixed);
        self.plan.projected.clear();
        if budget.projected == 0 {
            return;
        }
        for (index, slot) in panels.iter().enumerate() {
            let placement = self.plan.budget.placement(index);
            if placement.content == Place::Projected {
                self.plan.projected.extend(self.layers.projected(slot));
            }
            if placement.handles == Place::Projected {
                self.plan
                    .projected
                    .extend(handle_layers::placed(slot, active, head).map(Handle::projected));
            }
        }
    }

    // Ends the frame with the layers of the streamed scenes, the panels and the surfaces that keep
    // a layer of their own. The rays go over them. `panels` stand back to front. The flat panels,
    // the curved panels and the surfaces share that order: a panel in front of a surface shows over
    // it. The handles of a panel come right after the panel's own layers, so a panel in front of a
    // handle shows over it.
    fn end_with_layers(
        &mut self,
        display_time: xr::Time,
        panels: &[PanelSlot],
        head: Option<[f32; 3]>,
    ) -> Result<(), String> {
        let beams = self.pointers.beams();
        let active = self.pointers.handles();
        let spaces = self.headset.tracked_spaces(&self.xr_session);
        let (layers, surfaces, handles, budget) = (
            &self.layers,
            &self.surfaces,
            &self.handles,
            &self.plan.budget,
        );
        let shown = panels
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.ride != Ride::Hidden)
            .flat_map(move |(index, slot)| {
                let placement = budget.placement(index);
                // A panel that follows every move of the head or a controller rides in its space,
                // with its handles, so the compositor places them with the newest pose of it
                let place = slot.tracked.and_then(|tracked| spaces.located(tracked));
                let content = layers
                    .layer(slot)
                    .filter(|_| placement.content == Place::Own);
                let surfaces = surfaces
                    .layers(slot)
                    .filter(move |_| placement.surfaces == Place::Own);
                let handles = handles
                    .layers(slot, active, head)
                    .filter(move |_| placement.handles == Place::Own);
                content
                    .into_iter()
                    .chain(surfaces.map(Layer::Quad))
                    .chain(handles.map(Layer::Quad))
                    .map(move |layer| layer.in_space(place))
            });
        let rays = head
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
            self.trial.as_ref().map(TrialLayer::layer),
        )
    }

    // Asks the runtime for the model of each controller that it has not given one for, before the
    // frame that the runtime waits for, so that a question that takes a few milliseconds delays no
    // frame. A frame with no question due makes no call. The profiles are those of the last frame.
    fn ask_for_models(&mut self) {
        let profiles = Hand::BOTH.map(|hand| self.headset.controller(hand).profile);
        let offers = self.render_models.query(&self.xr_session, profiles);
        let states = self.space.volumes().controllers();
        for (hand, offer) in Hand::BOTH.into_iter().zip(offers) {
            if let Some(offer) = offer {
                states.set_offer(hand, offer);
            }
        }
    }

    // Puts a model at the grip of each controller that is in use, and takes the model of a hand
    // that holds none out of the scene.
    fn show_controllers(&self) {
        self.space
            .volumes()
            .controllers()
            .track(Hand::BOTH.map(|hand| self.headset.controller(hand)));
    }

    // The room follows what the app prefers, and the app learns what the room shows.
    fn follow_environment(&mut self) {
        let passthrough = &mut self.xr_session.passthrough;
        passthrough.set_opacity(self.environment.preference().unwrap_or(0.0));
        self.environment.show(passthrough.opacity());
    }
}

// Installs the keyboard that a focused text field opens: the system's where the runtime shows it,
// else the one that Cranpose draws. The first composition made the keyboard, because its panel asks
// for it.
fn install_keyboard(
    host: &Host,
    space: &Space,
    guest: &mut GuestView,
) -> (SoftKeyboard, Option<SystemKeyboard>) {
    let keyboard = space.keyboard();
    let system_keyboard = host.system_keyboard();
    match &system_keyboard {
        Some(system) => system.install(guest),
        None => keyboard.install(guest.shell()),
    }
    (keyboard, system_keyboard)
}
