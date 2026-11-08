// What a window target drew of each record of its scene, and the damage the
// scene's changes since make to it. Each frame reads the scene's journal and
// puts the areas the changed records were drawn in into the frame's damage;
// after the collect, which records how it drew each record, it adds the
// areas they are drawn in now.

use cranpose_core::{RecordId, RecordRef};
use cranpose_render_common::{
    graph_scene::Scene,
    scene_store::{
        BLUR_PIXEL_SLACK, JournalCursor, JournalEntry, JournalRead, Reach, RecordEvent,
        RecordEvents, SceneStore,
    },
};
use cranpose_ui_graphics::{Point, Rect};

use crate::{
    collect::{LayerScene, ShownArea},
    frame::{
        COPY_HOLD_OFF, Capture, Damage, DamageRects, DeviceRect, collect_captures, settle, widen,
    },
};

// The pixels a record's area takes in a target, whole pixels from the
// first to past the last.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PixelBox {
    x0: u16,
    y0: u16,
    x1: u16,
    y1: u16,
}

impl PixelBox {
    // The pixels of `viewport` that `rect`, in logical units at `scale`
    // pixels each, may touch: a pixel past it all around for filtering and
    // snapping, and under a scale of one the blur pixels a reach leaves out;
    // `None` when it touches none.
    fn of(rect: Rect, scale: f32, (width, height): (u32, u32)) -> Option<Self> {
        let margin = 2.0 + BLUR_PIXEL_SLACK * (1.0 - scale).max(0.0);
        let pixel = |value: f32, most: u32| value.clamp(0.0, most as f32) as u16;
        let x0 = pixel((rect.x * scale - margin).floor(), width);
        let y0 = pixel((rect.y * scale - margin).floor(), height);
        let x1 = pixel(((rect.x + rect.width) * scale + margin).ceil(), width);
        let y1 = pixel(((rect.y + rect.height) * scale + margin).ceil(), height);
        (x0 < x1 && y0 < y1).then_some(Self { x0, y0, x1, y1 })
    }

    fn intersect(self, other: Self) -> Option<Self> {
        let x0 = self.x0.max(other.x0);
        let y0 = self.y0.max(other.y0);
        let x1 = self.x1.min(other.x1);
        let y1 = self.y1.min(other.y1);
        (x0 < x1 && y0 < y1).then_some(Self { x0, y0, x1, y1 })
    }

    fn rect(self) -> DeviceRect {
        DeviceRect {
            x: f32::from(self.x0),
            y: f32::from(self.y0),
            width: f32::from(self.x1 - self.x0),
            height: f32::from(self.y1 - self.y0),
        }
    }
}

// How the target drew a record's subtree.
#[derive(Clone, Copy, Debug, Default)]
enum Area {
    // In place, within bounds the walk did not know.
    #[default]
    Unknown,
    Hidden,
    Drawn(PixelBox),
    // Through the record's own surface: a change beneath it changes all of
    // it.
    Composite(PixelBox),
}

impl Area {
    fn of(shown: ShownArea, scale: f32, viewport: (u32, u32)) -> Self {
        let pixels = |rect| PixelBox::of(rect, scale, viewport);
        match shown {
            ShownArea::Drawn(rect) => pixels(rect).map_or(Self::Hidden, Self::Drawn),
            ShownArea::Composite(rect) => pixels(rect).map_or(Self::Hidden, Self::Composite),
            ShownArea::Hidden => Self::Hidden,
            ShownArea::Unknown => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Shown {
    generation: u32,
    area: Area,
    // Where the record drew its own draws, beside its children.
    own: Area,
    // Where the origin of the record's space lay, when the target drew the
    // record in place and only moved its space there.
    origin: Option<Point>,
}

impl Shown {
    // The part of the record's own area that `within`, in the record's
    // space, covers.
    fn own_part(self, within: Reach, setting: TargetSetting) -> Area {
        let (Reach::Finite(rect), Some(origin)) = (within, self.origin) else {
            return if within == Reach::Empty {
                Area::Hidden
            } else {
                self.own
            };
        };
        let part = PixelBox::of(
            rect.translate(origin.x, origin.y),
            setting.root_scale,
            setting.viewport,
        );
        match (self.own, part) {
            (_, None) | (Area::Hidden, _) => Area::Hidden,
            (Area::Drawn(own), Some(part)) => part.intersect(own).map_or(Area::Hidden, Area::Drawn),
            (Area::Unknown, Some(part)) => Area::Drawn(part),
            (own @ Area::Composite(_), Some(_)) => own,
        }
    }
}

// How a culled walk takes a record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Visit {
    // It leaves the record out: it lies outside the region, as it did.
    Skip,
    // It walks the record, culling beneath it.
    Walk,
    // It walks all the record's subtree: the subtree moved or is new.
    WalkAll,
}

// What a window's walk leaves out: the records that did not change and lie
// outside a region, as the target last drew them.
pub(crate) struct Cull<'a> {
    region: DamageRects,
    shown: &'a [Shown],
    marks: &'a [(u32, Visit)],
    stamp: u32,
}

impl Cull<'_> {
    // How the walk takes `record`.
    pub(crate) fn visit(&self, record: RecordRef) -> Visit {
        let row = record.id.index();
        if let Some(&(stamp, visit)) = self.marks.get(row)
            && stamp == self.stamp
        {
            return visit;
        }
        let Some(shown) = self
            .shown
            .get(row)
            .filter(|shown| shown.generation == record.generation)
        else {
            return Visit::Walk;
        };
        match shown.area {
            Area::Drawn(pixels) | Area::Composite(pixels) if !self.region.meets(pixels.rect()) => {
                Visit::Skip
            }
            Area::Hidden => Visit::Skip,
            Area::Drawn(_) | Area::Composite(_) | Area::Unknown => Visit::Walk,
        }
    }
}

// What of a record a change reaches.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Scope {
    // All the record's subtree draws.
    Subtree,
    // What the record draws of its own, within the reach in its space.
    Own(Reach),
}

impl Scope {
    fn of(event: RecordEvent) -> Self {
        match event {
            RecordEvent::Redrawn(within) => Self::Own(within),
            _ => Self::Subtree,
        }
    }
}

/// What draws every pixel of a frame beside its records: a frame of another
/// setting than the frame before draws whole.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TargetSetting {
    pub(crate) viewport: (u32, u32),
    pub(crate) root_scale: f32,
    pub(crate) clear: wgpu::Color,
}

/// The most frames in a row a target whose frames keep changing most of
/// the window skips working out their damage: about four seconds at 60 Hz.
const MAX_SKIPPED: u32 = 256;
/// The frames to skip at or past which a target lets its areas go until it
/// records them again: after four frames in a row changed most of the
/// window.
const RELEASE_SKIPS: u32 = 64;

/// How a frame went into a target that keeps its frames.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TargetDrawn {
    /// What the frame changed in the target.
    pub(crate) damage: Damage,
    /// Whether the packet was one to skip working out its damage.
    pub(crate) skipped: bool,
    /// Whether the packet knew the frame before it.
    pub(crate) knew_before: bool,
    /// Whether the target is images that come cleared, whose frames the
    /// renderer's copy keeps.
    pub(crate) cleared: bool,
}

#[derive(Default)]
pub(crate) struct TargetState {
    cursor: Option<JournalCursor>,
    // By record row, how the target last drew the record.
    shown: Vec<Shown>,
    setting: Option<TargetSetting>,
    damage: DamageRects,
    // Whether the frame changes the whole window: an area it changes is not
    // known.
    whole: bool,
    // Whether the frame before is not known: the frame draws whole, and that
    // says nothing of how much of the window it changes.
    unknown_before: bool,
    // The records the frame's changes leave and what of each they reach,
    // whose new areas the frame adds after its collect.
    changed: Vec<(RecordRef, Scope)>,
    // The damage of the old areas, and whether one is not known, which each
    // end of the frame starts from.
    begun: (DamageRects, bool),
    // By row, the frame that marked the record and how a culled walk of
    // that frame takes it.
    marks: Vec<(u32, Visit)>,
    stamp: u32,
    path: Vec<RecordId>,
    captures: Vec<Capture>,
    // While frames keep changing most of the window, the frames left to
    // skip: 1, 4, 16, 64 and then [`MAX_SKIPPED`] after each such frame.
    skipped: u32,
    // The frames in a row that changed most of the window.
    full_frames: u32,
    // Whether the areas describe a frame before skipped ones, so the next
    // frame that records them draws whole.
    stale: bool,
    // Whether the frame in progress records its areas and works out its
    // damage.
    recording: bool,
    // Whether the frame in progress, skipped, changes nothing at all.
    still: bool,
    // The root the frame before drew.
    last_root: Option<RecordRef>,
    // When the records' changes since the frame before are lost, the area
    // the frame before drew the root in.
    lost_root: Option<Area>,
}

/// The records a target state follows: a scene's, or those of a store of
/// their own.
pub(crate) enum Journaled<'a> {
    Scene(&'a mut Scene),
    Store(&'a mut SceneStore),
}

impl Journaled<'_> {
    fn open(&mut self) -> JournalCursor {
        match self {
            Self::Scene(scene) => scene.open_journal(RecordEvents::ALL),
            Self::Store(store) => store.open_journal(RecordEvents::ALL),
        }
    }

    fn follow(&mut self, cursor: &mut JournalCursor, events: RecordEvents) {
        match self {
            Self::Scene(scene) => scene.follow_journal(cursor, events),
            Self::Store(store) => store.follow_journal(cursor, events),
        }
    }

    fn read<R>(
        &mut self,
        cursor: &mut JournalCursor,
        read: impl FnOnce(&SceneStore, JournalRead<'_>) -> R,
    ) -> R {
        match self {
            Self::Scene(scene) => scene.read_journal(cursor, read),
            Self::Store(store) => store.read_journal(cursor, read),
        }
    }
}

impl TargetState {
    /// Ends a run of frames that changed most of the window: the app held
    /// still since the frame before.
    pub(crate) fn held_still(&mut self) {
        self.skipped = 0;
        self.full_frames = 0;
    }

    /// Starts a frame of `setting` of the records of `source`: reads their
    /// changes since the frame before, and puts the areas the frame before
    /// drew the changed records in into the frame's damage.
    pub(crate) fn begin(&mut self, mut source: Journaled<'_>, setting: TargetSetting) {
        self.damage = DamageRects::default();
        self.changed.clear();
        let mut cursor = self.cursor.take().unwrap_or_else(|| source.open());
        let recording = self.skipped == 0;
        if recording != self.recording {
            // A frame that skips working out its damage needs no area of a
            // redraw, so the scene works out none while frames skip.
            let events = if recording {
                RecordEvents::ALL
            } else {
                RecordEvents::ALL_WITHOUT_REACH
            };
            source.follow(&mut cursor, events);
        }
        self.recording = recording;
        if !self.recording {
            // A frame that changes nothing says so, skipped or not.
            let same = self.setting == Some(setting);
            self.still = source.read(&mut cursor, |_, read| {
                same && matches!(read, JournalRead::Changes(entries) if entries.is_empty())
            });
            self.cursor = Some(cursor);
            return;
        }
        self.whole = false;
        self.lost_root = None;
        self.unknown_before = std::mem::take(&mut self.stale) || self.setting != Some(setting);
        self.setting = Some(setting);
        source.read(&mut cursor, |store, read| match read {
            // The records went or were replaced: all the root drew before
            // and draws now changes.
            JournalRead::Lost => {
                self.lost_root = Some(
                    self.last_root
                        .and_then(|root| self.shown(root))
                        .map_or(Area::Hidden, |shown| shown.area),
                );
            }
            JournalRead::Changes(entries) => {
                for entry in entries {
                    if self.whole || self.unknown_before {
                        break;
                    }
                    self.note(store, entry);
                }
            }
        });
        self.cursor = Some(cursor);
        self.begun = (self.damage, self.whole);
    }

    // The damage of the areas the frame before drew the changed records in;
    // `None` when the frame works out no damage or one of them is not
    // known.
    pub(crate) fn begun(&self) -> Option<DamageRects> {
        let known = self.recording && !self.unknown_before && self.lost_root.is_none();
        (known && !self.begun.1).then_some(self.begun.0)
    }

    // What a walk of the frame leaves out of `region` of the records of
    // `store`: it walks every changed record and its ancestors, and all of
    // a moved or new record's subtree.
    pub(crate) fn cull(&mut self, store: &SceneStore, region: DamageRects) -> Cull<'_> {
        self.stamp = self.stamp.wrapping_add(1).max(1);
        for at in 0..self.changed.len() {
            let (record, scope) = self.changed[at];
            let Some(id) = store.resolve(record) else {
                continue;
            };
            let visit = match scope {
                Scope::Subtree => Visit::WalkAll,
                Scope::Own(_) => Visit::Walk,
            };
            let mut next = self.mark(id.index(), visit).then_some(id);
            while let Some(parent) = next.and_then(|id| store.parent(id)) {
                next = self.mark(parent.index(), Visit::Walk).then_some(parent);
            }
        }
        Cull {
            region,
            shown: &self.shown,
            marks: &self.marks,
            stamp: self.stamp,
        }
    }

    // Marks the record at `row` for the frame's walk to take as `visit` at
    // least; returns whether the frame marked it first now.
    fn mark(&mut self, row: usize, visit: Visit) -> bool {
        if row >= self.marks.len() {
            self.marks.resize(row + 1, (0, Visit::Skip));
        }
        let mark = &mut self.marks[row];
        let first = mark.0 != self.stamp;
        *mark = (self.stamp, if first { visit } else { mark.1.max(visit) });
        first
    }

    // Puts the area the frame before drew the record of `entry` in into the
    // damage, and keeps the record to add its area after the collect.
    fn note(&mut self, store: &SceneStore, entry: &JournalEntry) {
        let scope = Scope::of(entry.event);
        if !matches!(entry.event, RecordEvent::Inserted | RecordEvent::Claimed) {
            let area = self.area_of(store, (entry.record, scope), entry.parent);
            self.add(area);
        }
        if entry.event != RecordEvent::Removed {
            self.changed.push((entry.record, scope));
        }
    }

    /// Whether the frame records how it draws each record: a frame skipped
    /// while frames keep changing most of the window does not.
    pub(crate) fn records(&self) -> bool {
        self.recording
    }

    /// Ends the frame whose collect drew `root`: keeps how it drew each
    /// record, adds the areas the changed records are drawn in now, and
    /// returns the frame's damage.
    pub(crate) fn end(&mut self, store: &SceneStore, root: Option<&mut LayerScene>) -> Damage {
        if !self.recording {
            return if self.still {
                Damage::Part(DamageRects::default())
            } else {
                Damage::Skipped
            };
        }
        let damage = self.ended(store, root);
        self.last_root = store.root().map(|root| store.record_ref(root));
        damage
    }

    /// Whether the frame in progress knew the frame before it: one that did
    /// not draws whole, as the frame later frames compare with.
    pub(crate) fn knew_before(&self) -> bool {
        !self.recording || !self.unknown_before
    }

    /// Follows how the renderer drew a frame into a target that keeps its
    /// frames. A frame skipped counts towards the frames to skip. After a
    /// frame that changed the whole target, though its packet knew the frame
    /// before, frames skip working out their damage: 1, 4, 16, 64 and then
    /// [`MAX_SKIPPED`] frames, each such frame in a row quadrupling them,
    /// from 64 frames on for images that come cleared; past
    /// [`RELEASE_SKIPS`], the areas go until a frame records them again.
    pub(crate) fn drawn(&mut self, drawn: TargetDrawn) {
        if drawn.skipped {
            self.skipped = self.skipped.saturating_sub(1);
            return;
        }
        match drawn.damage {
            Damage::Full if drawn.knew_before => {}
            Damage::Full | Damage::Skipped => return,
            Damage::Part(_) => {
                self.full_frames = 0;
                return;
            }
        }
        self.full_frames = self.full_frames.saturating_add(1);
        let first = if drawn.cleared {
            COPY_HOLD_OFF.trailing_zeros()
        } else {
            0
        };
        let doublings = 2 * (self.full_frames - 1) + first;
        self.skipped = 1u32 << doublings.min(MAX_SKIPPED.trailing_zeros());
        self.stale = true;
        if self.skipped >= RELEASE_SKIPS {
            self.shown = Vec::new();
            self.changed = Vec::new();
            self.path = Vec::new();
            self.captures = Vec::new();
        }
    }

    fn ended(&mut self, store: &SceneStore, mut root: Option<&mut LayerScene>) -> Damage {
        let Some(setting) = self.setting else {
            return Damage::Full;
        };
        (self.damage, self.whole) = self.begun;
        let shown = root.as_deref_mut().map(|root| root.shown.drain(..));
        for shown in shown.into_iter().flatten() {
            let row = shown.record.id.index();
            if row >= self.shown.len() {
                self.shown.resize(row + 1, Shown::default());
            }
            self.shown[row] = Shown {
                generation: shown.record.generation,
                area: Area::of(shown.area, setting.root_scale, setting.viewport),
                own: Area::of(shown.own, setting.root_scale, setting.viewport),
                origin: shown.origin,
            };
        }
        if self.unknown_before {
            return Damage::Full;
        }
        if let Some(before) = self.lost_root {
            let now = store
                .root()
                .and_then(|root| self.shown(store.record_ref(root)))
                .map_or(Area::Hidden, |shown| shown.area);
            self.add(before);
            self.add(now);
        }
        for at in 0..self.changed.len() {
            if self.whole {
                break;
            }
            let (record, scope) = self.changed[at];
            if store.resolve(record).is_some() {
                let area = self.area_of(store, (record, scope), None);
                self.add(area);
            }
        }
        if self.whole {
            return Damage::Full;
        }
        if let Some(root) = root.filter(|root| !self.damage.is_empty() && root.contains_backdrop())
        {
            self.captures.clear();
            let place = (setting.root_scale, setting.viewport);
            if !collect_captures(root, place, &mut self.captures) {
                return Damage::Full;
            }
            widen(&self.captures, &mut self.damage);
        }
        settle(self.damage, setting.viewport)
    }

    // The area the target drew `record` in, as the target's areas say: that
    // of its topmost ancestor drawn hidden or through a surface, else its
    // own. A record gone from the store is found under `parent`; one whose
    // parent went too lies in the area of that parent.
    fn area_of(
        &mut self,
        store: &SceneStore,
        (record, scope): (RecordRef, Scope),
        parent: Option<RecordRef>,
    ) -> Area {
        let mut current = match store.resolve(record) {
            Some(id) => store.parent(id),
            None => match parent {
                Some(parent) => match store.resolve(parent) {
                    Some(id) => Some(id),
                    None => return Area::Hidden,
                },
                None => None,
            },
        };
        self.path.clear();
        while let Some(id) = current {
            self.path.push(id);
            current = store.parent(id);
        }
        for &ancestor in self.path.iter().rev() {
            match self
                .shown(store.record_ref(ancestor))
                .map(|shown| shown.area)
            {
                Some(Area::Drawn(_) | Area::Unknown) => {}
                Some(area) => return area,
                // An ancestor new since the frame: the record moved under
                // it, and its old parent's change holds where it was.
                None => return Area::Hidden,
            }
        }
        let Some(setting) = self.setting else {
            return Area::Unknown;
        };
        self.shown(record)
            .map_or(Area::Hidden, |shown| match scope {
                Scope::Subtree => shown.area,
                Scope::Own(within) => shown.own_part(within, setting),
            })
    }

    fn shown(&self, record: RecordRef) -> Option<Shown> {
        self.shown
            .get(record.id.index())
            .filter(|shown| shown.generation == record.generation)
            .copied()
    }

    fn add(&mut self, area: Area) {
        match area {
            Area::Drawn(pixels) | Area::Composite(pixels) => self.damage.add(pixels.rect()),
            Area::Hidden => {}
            Area::Unknown => self.whole = true,
        }
    }
}
