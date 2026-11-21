//! The panels a headset shows and the controllers' cursor on them.
//!
//! In a headset every panel of the app is drawn by one composition into one texture, the atlas,
//! one panel under the other in the order the panels registered. Each panel becomes a layer of
//! the headset's compositor that shows its part of the atlas. A panel keeps its part while its
//! size and the sizes of the panels above it stay. When a panel changes its size or leaves, the
//! panels stack again from the top, so the atlas needs no more room than its panels. A panel that
//! the user resizes holds more room than its size, so that the panels stack again once for each
//! step of growth and not in each frame.

use std::{
    cell::{Cell, OnceCell, RefCell},
    rc::Rc,
    time::Duration,
};

use cranpose::{ExternalSurface, MutableState, mutableStateOf};
use cranpose_core::CompositionLocal;

mod carry_snap;
#[cfg(any(test, headset))]
pub(crate) mod field_dock;
mod surface_delivery;

#[cfg(any(test, headset))]
use crate::spatial_store::SpatialStore;
use crate::{
    FollowTarget, Hand, METERS_PER_DP, MovePolicy, PanelHit, Pose, Ray, Recenter, ResizeLimits,
    ResizePolicy, SpatialMoveEvent, SpatialMoveEventType, SpatialResizeEvent,
    SpatialResizeEventType, StereoMode,
    dock::Dock,
    follow_origin::{FollowOrigins, Standing},
    locals::with_locals,
    sound_sources::SoundSources,
    stream_source::StreamSource,
    surface_input::{InputSink, PointerSink, SurfaceInput},
    volumes::{VolumeContent, Volumes},
};

// The space between two panels in the atlas, in dp, so the compositor's filter never reaches a
// neighbor's pixels.
const ATLAS_GAP: f32 = 8.0;

// The steps in dp by which a panel's room in the atlas grows while the user resizes it. Each
// growth stacks the panels again and makes the atlas and the panel's image anew, so a resize that
// grows a little at each frame makes them once for every ROOM_STEP dp of growth.
const ROOM_STEP: f32 = 128.0;

// The step in dp by which the dot of a cursor moves. A smaller move leaves the dot where it is.
const CURSOR_STEP: f32 = 0.5;

// Where a subspace starts: a meter in front of where the viewer's head was when the app
// started, at the same height.
pub(crate) const SUBSPACE_ORIGIN: Pose = Pose::at([0.0, 0.0, -1.0]);

// The origin of the subspace that holds a panel: a place in the room, or a following subspace
// whose origin moves in the frame loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Origin {
    // The origin's pose. In a following subspace it is the identity. The panel's pose is then
    // relative to the subspace's live origin.
    pose: Pose,
    // The following subspace that places the panel
    follower: Option<u64>,
}

impl Origin {
    // The origin of a following subspace.
    pub(crate) const fn following(id: u64) -> Self {
        Self {
            pose: Pose::IDENTITY,
            follower: Some(id),
        }
    }

    // The pose `child`, given in this origin's frame.
    pub(crate) fn then(&self, child: &Pose) -> Pose {
        self.pose.then(child)
    }

    // The following subspace that this origin belongs to, if it is one.
    pub(crate) const fn follower(&self) -> Option<u64> {
        self.follower
    }
}

impl From<Pose> for Origin {
    fn from(pose: Pose) -> Self {
        Self {
            pose,
            follower: None,
        }
    }
}

// What a panel shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PanelKind {
    // Cranpose content, drawn into the atlas.
    Content,
    // A surface that another producer writes into. It has no part of the atlas, and no
    // composition: a ray stops on it and presses nothing.
    Surface(StereoMode),
    // 3D content that the scene of the volumes draws. The panel's pose is the center of its box,
    // its size is the box's width and height in dp, and `depth` is the box's depth in dp. It has no
    // part of the atlas: a ray stops on the box.
    Volume { depth: f32 },
}

// A panel as the backend sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelSlot {
    pub(crate) id: u64,
    pub(crate) kind: PanelKind,
    // The panel's part of the atlas: left, top, width and height in dp. A surface or a volume holds
    // its size here and no room in the atlas.
    pub(crate) atlas: [f32; 4],
    // The panel's center and turn in the room.
    pub(crate) pose: Pose,
    // Whether the user may carry the panel with a controller.
    pub(crate) mobility: Mobility,
    // The shape the panel shows on. A surface panel is flat.
    pub(crate) shape: Shape,
    // How strongly the panel holds a layer of the compositor of its own
    pub(crate) priority: i32,
    // The sizes that the user may give the panel, if the content set a resize.
    pub(crate) resize: Option<ResizeLimits>,
    // The width and height in dp of the room that the panel holds in the atlas and in its image:
    // its size, or more while the user resizes it.
    pub(crate) room: [f32; 2],
    // Whether a controller places the panel
    pub(crate) ride: Ride,
}

impl PanelSlot {
    // The panel's width and height in dp.
    pub(crate) fn size(&self) -> [f32; 2] {
        [self.atlas[2], self.atlas[3]]
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The pose of the flat face that the panel's handles stand at and that a resize measures on:
    // the panel's own pose, or for a volume the pose of its front face, which lies half the box's
    // depth toward the viewer. A handle at the center plane of a deep box would lie behind a ray's
    // meeting with the box.
    pub(crate) fn face(&self) -> Pose {
        match self.kind {
            PanelKind::Volume { depth } => {
                self.pose
                    .then(&Pose::at([0.0, 0.0, depth * METERS_PER_DP / 2.0]))
            }
            PanelKind::Content | PanelKind::Surface(_) => self.pose,
        }
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The sizes that the user may give the panel with its corner handle, or `None` for a panel
    // that has none. A panel on a curve has none: its corner does not lie on its tangent plane.
    pub(crate) fn resize_limits(&self) -> Option<ResizeLimits> {
        self.resize.filter(|_| self.shape == Shape::Flat)
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Where `ray` meets the panel as it shows: flat or on its curve.
    pub(crate) fn hit(&self, ray: &Ray) -> Option<PanelHit> {
        let size = self.size().map(|dp| dp * METERS_PER_DP);
        match (self.kind, self.shape) {
            (PanelKind::Volume { depth }, _) => {
                ray.hit_box(&self.pose, [size[0], size[1], depth * METERS_PER_DP])
            }
            (_, Shape::Flat) => ray.hit_panel(&self.pose, size),
            (_, Shape::Curved { radius }) => ray.hit_cylinder(&self.pose, size, radius),
        }
    }
}

// The shape a panel shows on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Shape {
    Flat,
    // A cylinder of `radius` meters. Its upright axis stands that far from the panel's center
    // along the panel's +z, and the panel's width is arc length.
    Curved { radius: f32 },
}

// How a following subspace places a panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ride {
    // No controller places the panel.
    Free,
    // The panel stands where the controller of this hand puts it.
    On(Hand),
    // The panel's subspace has no place: it follows a controller that the runtime does not
    // track. The panel shows nowhere and takes no hits.
    Hidden,
}

// Whether the user may carry a panel with a controller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mobility {
    Fixed,
    Movable,
}

// Where a controller points on a panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Cursor {
    pub(crate) panel: u64,
    // The place on the panel in dp from its top left corner.
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) pressed: bool,
}

// The dot that shows a cursor on a panel: its place in dp from the panel's top left corner at
// the cursor's steps, and whether the trigger is down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dot {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) pressed: bool,
}

// A step of the user's carry of a panel, in the room's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelMove {
    pub(crate) panel: u64,
    pub(crate) event_type: SpatialMoveEventType,
    pub(crate) pose: Pose,
}

// A step of the user's resize of a panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelResize {
    pub(crate) panel: u64,
    pub(crate) event_type: SpatialResizeEventType,
    // The panel's size in dp
    pub(crate) size: [f32; 2],
}

// A panel of the app and the states that its composition reads while it draws. A change of a
// state moves or draws the panel again without composing its content again.
#[derive(Clone, Copy)]
pub(crate) struct Panel {
    pub(crate) id: u64,
    // Where the panel's part of the atlas starts in dp, from the atlas's top left corner.
    pub(crate) origin: MutableState<[f32; 2]>,
    // The dot of each hand's cursor on the panel, left then right, or `None` while that hand's
    // controller does not point at it.
    pub(crate) dots: MutableState<[Option<Dot>; 2]>,
    // The surface of a surface panel once the backend has made it.
    pub(crate) surface: MutableState<Option<ExternalSurface>>,
    // The pose of a volume in the room, which the scene of the volumes reads while it draws.
    pub(crate) volume_pose: MutableState<Pose>,
}

// The panels of the app in a headset. The headset's backend provides it around the app's
// content, and the spatial composables register their panels in it. Outside a headset there
// is none, and the composables draw flat.
#[derive(Clone)]
pub(crate) struct Space {
    inner: Rc<Inner>,
}

struct Inner {
    entries: RefCell<Vec<Entry>>,
    // The streamed scenes, which take no place in the room and no part of the atlas
    streams: RefCell<Vec<(u64, Rc<StreamSource>)>>,
    // Where following subspaces stand
    follow_origins: RefCell<FollowOrigins>,
    // The point sources of sound
    sounds: RefCell<SoundSources>,
    next_id: Cell<u64>,
    // Whether the headset shows curved panels. If not, they show and answer rays as flat ones.
    curves: Cell<bool>,
    // The content of the volumes and the camera that draws them, made when the first composable
    // that needs them asks, because their states belong to a composition's runtime
    volumes: OnceCell<Volumes>,
    // The panel that holds the focused text field, which the panels below a field stand under
    #[cfg(any(test, headset))]
    field_panel: Cell<Option<u64>>,
    // The keyboard that the headset draws for a focused text field, made when the headset's
    // content asks, because its state belongs to a composition's runtime
    #[cfg(any(test, headset))]
    keyboard: OnceCell<crate::soft_keyboard::SoftKeyboard>,
    // The planes that a movable panel snaps to, once the headset's content made the store
    #[cfg(any(test, headset))]
    spatial: OnceCell<Rc<SpatialStore>>,
    // The size in pixels of one eye's picture of the volumes, in logical pixels
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset's scene reads it")
    )]
    scene_size: [f32; 2],
}

// A registered panel as the space keeps it.
struct Entry {
    slot: PanelSlot,
    panel: Panel,
    // What the panel's dots state holds, so that a comparison needs no read of the state.
    dots: [Option<Dot>; 2],
    // The origin of the panel's subspace. A move reports poses relative to it.
    origin: Pose,
    // Where the user left the panel, relative to `origin`, or `None` while the panel's modifier
    // places it. The panel keeps its place in its subspace when the origin moves.
    carried: Option<Pose>,
    // The pose that the move in progress reported last, relative to `origin`
    moving: Option<Pose>,
    // The following subspace that places the panel, with the pose that the panel's modifier gives
    // relative to the subspace's origin
    follow: Option<Following>,
    // What the panel does when the user carries it, or `None` for a panel that stays
    policy: Option<MovePolicy>,
    // Who hears of the user's resize, or `None` for a panel that has no corner handle
    resize_policy: Option<ResizePolicy>,
    // Whether the user resizes the panel now: from the grab to the release
    resizing: bool,
    // For a surface: where the app takes the pointer's events, and what the surface knows of
    // each hand's pointer, left then right
    sink: Option<PointerSink>,
    // For a volume: where the app takes the ray's input events
    input_sink: Option<InputSink>,
    input: [SurfaceInput; 2],
    // Whether the panel stands below the panel of the focused text field, as the keyboard does,
    // wherever that panel stands
    dock: Dock,
}

// The place of a panel in a following subspace.
#[derive(Clone, Copy)]
struct Following {
    id: u64,
    local: Pose,
}

impl Entry {
    // Gives the scene of the volumes the pose of a volume, which draws it there without
    // composing. Other panels have no use for the state.
    fn publish_pose(&self) {
        if matches!(self.slot.kind, PanelKind::Volume { .. }) {
            self.panel.volume_pose.set(self.slot.pose);
        }
    }

    // Puts the panel where its following subspace stands, or hides it while the subspace has no
    // place.
    fn stand(&mut self, standing: Standing) {
        let Some(follow) = self.follow else {
            return;
        };
        match standing.at {
            Some(origin) => {
                self.origin = origin;
                self.slot.pose = origin.then(&self.carried.unwrap_or(follow.local));
                self.slot.ride = standing.hand.map_or(Ride::Free, Ride::On);
                self.publish_pose();
            }
            None => self.slot.ride = Ride::Hidden,
        }
    }
}

impl PartialEq for Space {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Space {
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // A space with no panels, whose volumes the headset draws in pictures of `scene_size` pixels
    // for each eye.
    pub(crate) fn with_scene_size(scene_size: [f32; 2]) -> Self {
        Self {
            inner: Rc::new(Inner {
                entries: RefCell::new(Vec::new()),
                streams: RefCell::new(Vec::new()),
                follow_origins: RefCell::new(FollowOrigins::default()),
                sounds: RefCell::new(SoundSources::default()),
                next_id: Cell::new(1),
                curves: Cell::new(true),
                volumes: OnceCell::new(),
                #[cfg(any(test, headset))]
                field_panel: Cell::new(None),
                #[cfg(any(test, headset))]
                keyboard: OnceCell::new(),
                #[cfg(any(test, headset))]
                spatial: OnceCell::new(),
                scene_size,
            }),
        }
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the tests make spaces that draw no volumes")
    )]
    // A space with no panels and no room for volumes.
    pub(crate) fn new() -> Self {
        Self::with_scene_size([0.0, 0.0])
    }

    // The content of the volumes and the camera that draws them.
    pub(crate) fn volumes(&self) -> Volumes {
        *self.inner.volumes.get_or_init(Volumes::new)
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset's scene calls it")
    )]
    // The size in logical pixels of one eye's picture of the volumes.
    pub(crate) fn scene_size(&self) -> [f32; 2] {
        self.inner.scene_size
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // How many volumes the app has now.
    pub(crate) fn volume_count(&self) -> usize {
        self.inner
            .entries
            .borrow()
            .iter()
            .filter(|entry| matches!(entry.slot.kind, PanelKind::Volume { .. }))
            .count()
    }

    // A new id for a streamed scene.
    pub(crate) fn new_stream_id(&self) -> u64 {
        let id = self.inner.next_id.get();
        self.inner.next_id.set(id + 1);
        id
    }

    // Registers the streamed scene `id`, or replaces its source.
    pub(crate) fn set_stream(&self, id: u64, source: Rc<StreamSource>) {
        let mut streams = self.inner.streams.borrow_mut();
        match streams.iter_mut().find(|(known, _)| *known == id) {
            Some((_, old)) => *old = source,
            None => streams.push((id, source)),
        }
    }

    // Forgets the streamed scene `id`.
    pub(crate) fn remove_stream(&self, id: u64) {
        self.inner
            .streams
            .borrow_mut()
            .retain(|(known, _)| *known != id);
    }

    #[cfg_attr(
        not(all(headset, target_os = "android")),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The streamed scenes, in the order they registered.
    pub(crate) fn streams(&self) -> Vec<(u64, Rc<StreamSource>)> {
        self.inner.streams.borrow().clone()
    }

    // A new following subspace of `target`, placed at once if the headset tracks the target.
    // Returns its id.
    pub(crate) fn new_follow_origin(&self, target: FollowTarget) -> u64 {
        let id = self.inner.next_id.get();
        self.inner.next_id.set(id + 1);
        self.inner.follow_origins.borrow_mut().add(id, target);
        id
    }

    // Forgets the following subspace `id`.
    pub(crate) fn remove_follow_origin(&self, id: u64) {
        self.inner.follow_origins.borrow_mut().remove(id);
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Tells the following subspaces where the head and the controllers are in the frame at `time`,
    // and whether the runtime recentered the room. The panels of a subspace that moved take
    // their new poses.
    pub(crate) fn track(
        &self,
        head: Option<Pose>,
        controllers: [Option<Pose>; 2],
        time: Duration,
        recenter: Option<Recenter>,
    ) {
        let mut origins = self.inner.follow_origins.borrow_mut();
        if !origins.track(head, controllers, time, recenter) {
            return;
        }
        for entry in self.inner.entries.borrow_mut().iter_mut() {
            if let Some(standing) = entry.follow.and_then(|follow| origins.moved(follow.id)) {
                entry.stand(standing);
            }
        }
    }

    // A new panel with its states. A composable calls it inside a `remember`, so that the states
    // end with the composable.
    pub(crate) fn new_panel(&self) -> Panel {
        let id = self.inner.next_id.get();
        self.inner.next_id.set(id + 1);
        Panel {
            id,
            origin: mutableStateOf([0.0, 0.0]),
            dots: mutableStateOf([None; 2]),
            surface: mutableStateOf(None),
            volume_pose: mutableStateOf(Pose::IDENTITY),
        }
    }

    // Registers `panel` of `size` dp at `pose`, in a subspace whose origin is `origin`, and
    // returns its part of the atlas. A new panel and a panel of a new size stack every panel
    // again.
    pub(crate) fn place(
        &self,
        panel: Panel,
        size: [f32; 2],
        origin: impl Into<Origin>,
        pose: Pose,
    ) -> [f32; 4] {
        self.place_kind(panel, PanelKind::Content, size, origin.into(), pose)
    }

    // Registers the surface `panel` of `size` dp at `pose`, in a subspace whose origin is
    // `origin`. It takes no room in the atlas.
    pub(crate) fn place_surface(
        &self,
        panel: Panel,
        stereo_mode: StereoMode,
        size: [f32; 2],
        origin: impl Into<Origin>,
        pose: Pose,
    ) {
        self.place_kind(
            panel,
            PanelKind::Surface(stereo_mode),
            size,
            origin.into(),
            pose,
        );
    }

    // Registers the volume `panel` with a box of `size` dp (width, height and depth) centered at
    // `pose`, in a subspace whose origin is `origin`. It takes no room in the atlas.
    pub(crate) fn place_volume(
        &self,
        panel: Panel,
        size: [f32; 3],
        origin: impl Into<Origin>,
        pose: Pose,
    ) {
        self.place_kind(
            panel,
            PanelKind::Volume { depth: size[2] },
            [size[0], size[1]],
            origin.into(),
            pose,
        );
    }

    fn place_kind(
        &self,
        panel: Panel,
        kind: PanelKind,
        size: [f32; 2],
        subspace: Origin,
        pose: Pose,
    ) -> [f32; 4] {
        let origin = subspace.pose;
        let mut entries = self.inner.entries.borrow_mut();
        let (index, changed) = match entries.iter().position(|entry| entry.slot.id == panel.id) {
            Some(index) => {
                let entry = &mut entries[index];
                // Content that another subspace holds is placed by its modifier again: the place that
                // the user carried it to is relative to the origin of the subspace that held it before
                if entry.follow.map(|follow| follow.id) != subspace.follower {
                    entry.carried = None;
                    entry.moving = None;
                }
                entry.origin = origin;
                entry.slot.pose = entry.carried.map_or(pose, |carried| origin.then(&carried));
                entry.slot.kind = kind;
                let slot = &mut entry.slot;
                slot.atlas = [slot.atlas[0], slot.atlas[1], size[0], size[1]];
                let room = if entry.resizing && kind == PanelKind::Content {
                    grown(slot.room, size)
                } else {
                    size
                };
                // A room is the size or a multiple of ROOM_STEP, so two equal rooms compare equal
                #[expect(clippy::float_cmp, reason = "a room is not a computed float")]
                let changed = slot.room != room;
                slot.room = room;
                (index, changed)
            }
            None => {
                entries.push(Entry {
                    slot: PanelSlot {
                        id: panel.id,
                        kind,
                        atlas: [0.0, 0.0, size[0], size[1]],
                        pose,
                        mobility: Mobility::Fixed,
                        shape: Shape::Flat,
                        priority: 0,
                        resize: None,
                        room: size,
                        ride: Ride::Free,
                    },
                    panel,
                    dots: [None; 2],
                    origin,
                    carried: None,
                    moving: None,
                    follow: None,
                    policy: None,
                    resize_policy: None,
                    resizing: false,
                    sink: None,
                    input_sink: None,
                    input: [SurfaceInput::default(); 2],
                    dock: Dock::Free,
                });
                (entries.len() - 1, true)
            }
        };
        if changed {
            restack(&mut entries);
        }
        let entry = &mut entries[index];
        // The panel stands where its subspace's live origin puts it. A composition that draws
        // later in a frame must not move it back to the pose that the composition gave
        entry.follow = subspace.follower.map(|id| Following { id, local: pose });
        match subspace
            .follower
            .and_then(|id| self.inner.follow_origins.borrow().standing(id))
        {
            Some(standing) => entry.stand(standing),
            None => entry.slot.ride = Ride::Free,
        }
        entry.publish_pose();
        entry.slot.atlas
    }

    // Sets what the panel `id` does when the user carries it. A panel with no policy stays.
    pub(crate) fn set_move_policy(&self, id: u64, policy: Option<&MovePolicy>) {
        let mut entries = self.inner.entries.borrow_mut();
        if let Some(entry) = entries.iter_mut().find(|entry| entry.slot.id == id) {
            entry.slot.mobility = if policy.is_some() {
                Mobility::Movable
            } else {
                Mobility::Fixed
            };
            entry.policy = policy.cloned();
        }
    }

    // Sets the sizes that the user may give the panel `id` and who hears of the resize, or `None`
    // for a panel that has no corner handle. A panel that loses its handle ends a resize in
    // progress.
    pub(crate) fn set_resizable(&self, id: u64, resize: Option<(ResizeLimits, &ResizePolicy)>) {
        let mut entries = self.inner.entries.borrow_mut();
        let Some(entry) = entries.iter_mut().find(|entry| entry.slot.id == id) else {
            return;
        };
        entry.slot.resize = resize.map(|(limits, _)| limits);
        entry.resize_policy = resize.map(|(_, policy)| policy.clone());
        if resize.is_none() && entry.resizing {
            entry.resizing = false;
            settle(entry);
            restack(&mut entries);
        }
    }

    // Sets where the panel `id` stands in relation to the panel of the focused text field.
    pub(crate) fn set_dock(&self, id: u64, dock: Dock) {
        let mut entries = self.inner.entries.borrow_mut();
        if let Some(entry) = entries.iter_mut().find(|entry| entry.slot.id == id) {
            entry.dock = dock;
        }
    }

    // Gives the space the store whose planes a movable panel snaps to. The first store stays.
    #[cfg(any(test, headset))]
    pub(crate) fn attach_spatial_store(&self, store: &Rc<SpatialStore>) {
        self.inner.spatial.get_or_init(|| Rc::clone(store));
    }

    // The keyboard that the headset draws for a focused text field.
    #[cfg(any(test, headset))]
    pub(crate) fn keyboard(&self) -> crate::soft_keyboard::SoftKeyboard {
        self.inner
            .keyboard
            .get_or_init(crate::soft_keyboard::SoftKeyboard::new)
            .clone()
    }

    // The panel that holds the focused text field, or `None` while no panel does. The panels that
    // stand below the field's panel stand under it.
    #[cfg(any(test, headset))]
    pub(crate) fn set_field_panel(&self, panel: Option<u64>) {
        self.inner.field_panel.set(panel);
    }

    // The panel of content that covers the point `at` of the atlas, in dp. The panels that stand
    // below a field's panel are left out: they hold no field.
    #[cfg(any(test, headset))]
    pub(crate) fn panel_at(&self, at: [f32; 2]) -> Option<u64> {
        let [x, y] = at;
        self.inner
            .entries
            .borrow()
            .iter()
            .filter(|entry| entry.slot.kind == PanelKind::Content && entry.dock == Dock::Free)
            .find(|entry| {
                let [left, top, width, height] = entry.slot.atlas;
                (left..left + width).contains(&x) && (top..top + height).contains(&y)
            })
            .map(|entry| entry.slot.id)
    }

    // Sets the shape of the panel `id` and how strongly it holds a layer of its own.
    pub(crate) fn set_layer(&self, id: u64, shape: Shape, priority: i32) {
        let mut entries = self.inner.entries.borrow_mut();
        if let Some(entry) = entries.iter_mut().find(|entry| entry.slot.id == id) {
            entry.slot.shape = shape;
            entry.slot.priority = priority;
        }
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Sets whether the headset shows curved panels. The panels of a headset that does not show
    // them are flat.
    pub(crate) fn set_curves_shown(&self, shown: bool) {
        self.inner.curves.set(shown);
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Applies a step of a carry to its panel and reports it to the panel's policy. The panel
    // keeps the pose that the user gave until it leaves, unless the policy leaves the pose to the
    // app.
    pub(crate) fn carry(&self, step: &PanelMove) {
        let heard = {
            let mut entries = self.inner.entries.borrow_mut();
            let Some(entry) = entries.iter_mut().find(|entry| entry.slot.id == step.panel) else {
                return;
            };
            // A policy that snaps puts the carried pose on a plane near it, while the user moves
            let room = carry_snap::snapped(self, entry, step);
            let pose = entry.origin.inverse().then(&room);
            let previous = entry.moving.unwrap_or(pose);
            let (pose, previous) = match step.event_type {
                SpatialMoveEventType::Start => (pose, pose),
                SpatialMoveEventType::Moving => (pose, previous),
                SpatialMoveEventType::End => (previous, previous),
            };
            entry.moving = (step.event_type != SpatialMoveEventType::End).then_some(pose);
            if step.event_type == SpatialMoveEventType::Moving
                && entry.policy.as_ref().is_some_and(MovePolicy::moves_content)
            {
                entry.slot.pose = room;
                entry.carried = Some(pose);
                entry.publish_pose();
            }
            let event = SpatialMoveEvent {
                event_type: step.event_type,
                pose,
                previous_pose: previous,
                room_pose: entry.origin.then(&pose),
            };
            let listener = entry.policy.as_ref().and_then(MovePolicy::listener);
            listener.map(|listener| (listener, event))
        };
        // The callback runs with no borrow held, so that it may call the space
        if let Some((listener, event)) = heard {
            listener(event);
        }
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Reports a step of a resize to its panel's policy. The app applies the size: the panel keeps
    // its size until the content places it at another one. From the grab to the release the
    // panel's room in the atlas only grows, in steps. At the release it shrinks to the size that
    // the app applied.
    pub(crate) fn resize(&self, step: &PanelResize) {
        let heard = {
            let mut entries = self.inner.entries.borrow_mut();
            let Some(entry) = entries.iter_mut().find(|entry| entry.slot.id == step.panel) else {
                return;
            };
            entry.resizing = step.event_type != SpatialResizeEventType::End;
            let event = SpatialResizeEvent {
                event_type: step.event_type,
                size: step.size,
            };
            entry
                .resize_policy
                .as_ref()
                .map(|policy| (policy.listener(), event))
        };
        // The callback runs with no borrow held, so that it may call the space
        if let Some((listener, event)) = heard {
            listener(event);
        }
        if step.event_type == SpatialResizeEventType::End {
            let mut entries = self.inner.entries.borrow_mut();
            if let Some(entry) = entries.iter_mut().find(|entry| entry.slot.id == step.panel) {
                settle(entry);
            }
            restack(&mut entries);
        }
    }

    #[cfg_attr(
        not(all(headset, target_os = "android")),
        expect(dead_code, reason = "the Android headset backend calls it")
    )]
    // Gives the surface panel `id` the surface that the backend made for it.
    pub(crate) fn publish_surface(&self, id: u64, surface: ExternalSurface) {
        let entries = self.inner.entries.borrow();
        if let Some(entry) = entries.iter().find(|entry| entry.slot.id == id) {
            entry.panel.surface.set(Some(surface));
        }
    }

    // Forgets the panel `id`. The panels below it move up.
    pub(crate) fn remove(&self, id: u64) {
        let mut entries = self.inner.entries.borrow_mut();
        entries.retain(|entry| entry.slot.id != id);
        restack(&mut entries);
        if let Some(volumes) = self.inner.volumes.get() {
            volumes.remove(id);
        }
        self.inner.sounds.borrow_mut().remove(id);
    }

    // The scene of the volumes takes the content of the volume `content.id`, or replaces what it
    // took.
    pub(crate) fn publish_volume(&self, content: VolumeContent) {
        self.volumes().set(content);
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Puts the panels into `panels` in place of what it held. A frame loop passes the same list
    // at each frame, so the list keeps its storage.
    pub(crate) fn panels_into(&self, panels: &mut Vec<PanelSlot>) {
        let curves = self.inner.curves.get();
        let entries = self.inner.entries.borrow();
        panels.clear();
        panels.extend(entries.iter().map(|entry| PanelSlot {
            shape: if curves {
                entry.slot.shape
            } else {
                Shape::Flat
            },
            ..entry.slot
        }));
        #[cfg(any(test, headset))]
        field_dock::dock(&entries, self.inner.field_panel.get(), panels);
    }

    #[cfg(test)]
    pub(crate) fn panels(&self) -> Vec<PanelSlot> {
        let mut panels = Vec::new();
        self.panels_into(&mut panels);
        panels
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The width and height in dp that the atlas needs for every panel.
    pub(crate) fn atlas_size(&self) -> [f32; 2] {
        self.inner
            .entries
            .borrow()
            .iter()
            .filter(|entry| entry.slot.kind == PanelKind::Content)
            .fold([0.0, 0.0], |size, entry| {
                let [x, y, _, _] = entry.slot.atlas;
                let [width, height] = entry.slot.room;
                [size[0].max(x + width), size[1].max(y + height)]
            })
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Shows the cursor of `hand` on its panel and on no other. A panel draws again only when one
    // of its dots changes. A move below the dot's step changes nothing.
    pub(crate) fn set_cursor(&self, hand: Hand, cursor: Option<Cursor>) {
        for entry in self.inner.entries.borrow_mut().iter_mut() {
            let dot = cursor
                .filter(|cursor| cursor.panel == entry.slot.id)
                .map(dot_of);
            if entry.dots[hand.index()] != dot {
                entry.dots[hand.index()] = dot;
                entry.panel.dots.set(entry.dots);
            }
        }
    }
}

// Stacks the panels from the top of the atlas in the order they registered. A panel that moves
// gets its new origin in its state.
fn restack(entries: &mut [Entry]) {
    let mut top = 0.0;
    for entry in entries
        .iter_mut()
        .filter(|entry| entry.slot.kind == PanelKind::Content)
    {
        let [left, old_top, _, _] = entry.slot.atlas;
        let height = entry.slot.room[1];
        // Tops lie on even dp, so a change is always more than half a dp
        if (old_top - top).abs() > 0.5 {
            entry.slot.atlas[1] = top;
            entry.panel.origin.set([left, top]);
        }
        // Tops on even dp stay on whole pixels at the densities of one and a half and two
        top = ((top + height + ATLAS_GAP) / 2.0).ceil() * 2.0;
    }
}

// The room for a panel of `size` that holds `room` now: the same room if it holds the size,
// else the size rounded up to the next ROOM_STEP.
fn grown(room: [f32; 2], size: [f32; 2]) -> [f32; 2] {
    [0, 1].map(|axis| {
        if size[axis] <= room[axis] {
            room[axis]
        } else {
            (size[axis] / ROOM_STEP).ceil() * ROOM_STEP
        }
    })
}

// Gives the entry's panel the room of its size.
fn settle(entry: &mut Entry) {
    entry.slot.room = entry.slot.size();
}

fn dot_of(cursor: Cursor) -> Dot {
    let step = |dp: f32| (dp / CURSOR_STEP).round() * CURSOR_STEP;
    Dot {
        x: step(cursor.x),
        y: step(cursor.y),
        pressed: cursor.pressed,
    }
}

// The origin of the subspace that holds the content, in the room. The pose of a panel is relative
// to it.
pub(crate) fn local_subspace_origin() -> CompositionLocal<Origin> {
    with_locals(|locals| locals.origin.clone())
}

// The space of the headset the content shows in, or `None` outside a headset.
pub(crate) fn local_space() -> CompositionLocal<Option<Space>> {
    with_locals(|locals| locals.space.clone())
}

mod sounds;

#[cfg(test)]
#[path = "tests/space_tests.rs"]
mod tests;
