//! The panels a headset shows and the controllers' cursor on them.
//!
//! In a headset every panel of the app is drawn by one composition into one texture, the atlas,
//! one panel under the other in the order the panels registered. Each panel becomes a layer of
//! the headset's compositor that shows its part of the atlas. A panel keeps its part while its
//! size and the sizes of the panels above it stay. When a panel changes its size or leaves, the
//! panels stack again from the top, so the atlas needs no more room than its panels.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use cranpose::{ExternalSurface, MutableState, mutableStateOf};
use cranpose_core::CompositionLocal;

use crate::{
    FollowTarget, Hand, METERS_PER_DP, MovePolicy, PanelHit, Pose, Ray, Recenter, SpatialMoveEvent,
    SpatialMoveEventType, StereoMode,
    follow_origin::{FollowOrigins, Standing},
    locals::with_locals,
    stream_source::StreamSource,
    surface_input::{PointerSink, SurfaceGesture, SurfaceInput},
};

// The space between two panels in the atlas, in dp, so the compositor's filter never reaches a
// neighbor's pixels.
const ATLAS_GAP: f32 = 8.0;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PanelKind {
    // Cranpose content, drawn into the atlas.
    Content,
    // A surface that another producer writes into. It has no part of the atlas, and no
    // composition: a ray stops on it and presses nothing.
    Surface(StereoMode),
}

// A panel as the backend sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PanelSlot {
    pub(crate) id: u64,
    pub(crate) kind: PanelKind,
    // The panel's part of the atlas: left, top, width and height in dp. A surface holds its size
    // here and no room in the atlas.
    pub(crate) atlas: [f32; 4],
    // The panel's center and turn in the room.
    pub(crate) pose: Pose,
    // Whether the user may carry the panel with a controller.
    pub(crate) mobility: Mobility,
    // The shape the panel shows on. A surface panel is flat.
    pub(crate) shape: Shape,
    // Whether a controller places the panel
    pub(crate) ride: Ride,
}

impl PanelSlot {
    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // The panel's width and height in dp.
    pub(crate) fn size(&self) -> [f32; 2] {
        [self.atlas[2], self.atlas[3]]
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Where `ray` meets the panel as it shows: flat or on its curve.
    pub(crate) fn hit(&self, ray: &Ray) -> Option<PanelHit> {
        let size = self.size().map(|dp| dp * METERS_PER_DP);
        match self.shape {
            Shape::Flat => ray.hit_panel(&self.pose, size),
            Shape::Curved { radius } => ray.hit_cylinder(&self.pose, size, radius),
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

// A panel of the app and the states that its composition reads while it draws. A change of a
// state moves or draws the panel again without composing its content again.
#[derive(Clone, Copy)]
pub(crate) struct Panel {
    pub(crate) id: u64,
    // Where the panel's part of the atlas starts in dp, from the atlas's top left corner.
    pub(crate) origin: MutableState<[f32; 2]>,
    // The dot of the cursor on the panel, or `None` while no controller points at it.
    pub(crate) dot: MutableState<Option<Dot>>,
    // The surface of a surface panel once the backend has made it.
    pub(crate) surface: MutableState<Option<ExternalSurface>>,
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
    next_id: Cell<u64>,
    // Whether the headset shows curved panels. If not, they show and answer rays as flat ones.
    curves: Cell<bool>,
}

// A registered panel as the space keeps it.
struct Entry {
    slot: PanelSlot,
    panel: Panel,
    // What the panel's dot state holds, so that a comparison needs no read of the state.
    dot: Option<Dot>,
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
    // For a surface: where the app takes the pointer's events, and what the surface knows of
    // the pointer
    sink: Option<PointerSink>,
    input: SurfaceInput,
}

// The place of a panel in a following subspace.
#[derive(Clone, Copy)]
struct Following {
    id: u64,
    local: Pose,
}

impl Entry {
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
    // A space with no panels.
    pub(crate) fn new() -> Self {
        Self {
            inner: Rc::new(Inner {
                entries: RefCell::new(Vec::new()),
                streams: RefCell::new(Vec::new()),
                follow_origins: RefCell::new(FollowOrigins::default()),
                next_id: Cell::new(1),
                curves: Cell::new(true),
            }),
        }
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
            dot: mutableStateOf(None),
            surface: mutableStateOf(None),
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
                entry.origin = origin;
                entry.slot.pose = entry.carried.map_or(pose, |carried| origin.then(&carried));
                let slot = &mut entry.slot;
                let changed = slot.atlas[2..] != size;
                slot.atlas = [slot.atlas[0], slot.atlas[1], size[0], size[1]];
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
                        ride: Ride::Free,
                    },
                    panel,
                    dot: None,
                    origin,
                    carried: None,
                    moving: None,
                    follow: None,
                    policy: None,
                    sink: None,
                    input: SurfaceInput::default(),
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

    // Sets where the surface `id` sends the pointer's events, or `None` for a surface that takes
    // none.
    pub(crate) fn set_surface_sink(&self, id: u64, sink: Option<PointerSink>) {
        let mut entries = self.inner.entries.borrow_mut();
        if let Some(entry) = entries.iter_mut().find(|entry| entry.slot.id == id) {
            entry.sink = sink;
        }
    }

    // Sets the shape of the panel `id`.
    pub(crate) fn set_shape(&self, id: u64, shape: Shape) {
        let mut entries = self.inner.entries.borrow_mut();
        if let Some(entry) = entries.iter_mut().find(|entry| entry.slot.id == id) {
            entry.slot.shape = shape;
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
            let pose = entry.origin.inverse().then(&step.pose);
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
                entry.slot.pose = step.pose;
                entry.carried = Some(pose);
            }
            let event = SpatialMoveEvent {
                event_type: step.event_type,
                pose,
                previous_pose: previous,
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
    // Tells the surface that `gesture` names what the pointer did on it, as Compose's pointer
    // event. A surface that takes no pointer events sees nothing.
    pub(crate) fn deliver_to_surface(&self, gesture: &SurfaceGesture) {
        let heard = {
            let mut entries = self.inner.entries.borrow_mut();
            entries
                .iter_mut()
                .find(|entry| entry.slot.id == gesture.surface)
                .and_then(|entry| {
                    let sink = entry.sink.clone()?;
                    Some((sink, entry.input.event(gesture.gesture)?))
                })
        };
        // The callback runs with no borrow held, so that it may call the space
        if let Some((sink, event)) = heard {
            sink(event);
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
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Puts the panels into `panels` in place of what it held. A frame loop passes the same list
    // at each frame, so the list keeps its storage.
    pub(crate) fn panels_into(&self, panels: &mut Vec<PanelSlot>) {
        let curves = self.inner.curves.get();
        panels.clear();
        panels.extend(self.inner.entries.borrow().iter().map(|entry| PanelSlot {
            shape: if curves {
                entry.slot.shape
            } else {
                Shape::Flat
            },
            ..entry.slot
        }));
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
                let [x, y, width, height] = entry.slot.atlas;
                [size[0].max(x + width), size[1].max(y + height)]
            })
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Shows the cursor on its panel and on no other. A panel draws again only when its dot
    // changes. A move below the dot's step changes nothing.
    pub(crate) fn set_cursor(&self, cursor: Option<Cursor>) {
        for entry in self.inner.entries.borrow_mut().iter_mut() {
            let dot = cursor
                .filter(|cursor| cursor.panel == entry.slot.id)
                .map(dot_of);
            if entry.dot != dot {
                entry.dot = dot;
                entry.panel.dot.set(dot);
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
        let [left, old_top, _, height] = entry.slot.atlas;
        // Tops lie on even dp, so a change is always more than half a dp
        if (old_top - top).abs() > 0.5 {
            entry.slot.atlas[1] = top;
            entry.panel.origin.set([left, top]);
        }
        // Tops on even dp stay on whole pixels at the densities of one and a half and two
        top = ((top + height + ATLAS_GAP) / 2.0).ceil() * 2.0;
    }
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

#[cfg(test)]
#[path = "tests/space_tests.rs"]
mod tests;
