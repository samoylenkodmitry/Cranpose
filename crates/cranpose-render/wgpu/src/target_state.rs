// What a window target drew of each record of its scene, and the damage the
// scene's changes since make to it. Each frame reads the scene's journal and
// puts the areas the changed records were drawn in into the frame's damage;
// after the collect, which records how it drew each record, it adds the
// areas they are drawn in now.

use cranpose_core::{RecordRef, collections::growth_slack::GrowthSlack};
use cranpose_render_common::{
    scene_store::{
        BLUR_PIXEL_SLACK, JournalEntry, JournalRead, Reach, RecordEvent, RecordEvents, SceneStore,
        SceneVersion,
    },
    space::scene_of,
};
use cranpose_ui_graphics::{Point, Rect};

use crate::{
    collect::{LayerScene, ShownArea},
    frame::{
        Capture, Damage, DamageBuilder, DamageRects, DeviceRect, OwnSake, collect_captures, widen,
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

// The pixels of a target that logical rects take: `scale` pixels a unit, a
// `margin` of pixels past a rect all around, in a window `width` by `height`
// pixels, clamped to 65,535.
#[derive(Clone, Copy)]
struct PixelGrid {
    scale: f32,
    margin: f32,
    width: f32,
    height: f32,
}

impl PixelGrid {
    // The grid of `setting`: a pixel past a rect for filtering and
    // snapping, and under a scale of one the blur pixels a reach leaves out.
    fn of(setting: TargetSetting) -> Self {
        let scale = setting.root_scale;
        let (width, height) = setting.viewport;
        let most = |pixels: u32| pixels.min(u32::from(u16::MAX)) as f32;
        Self {
            scale,
            margin: 2.0 + BLUR_PIXEL_SLACK * (1.0 - scale).max(0.0),
            width: most(width),
            height: most(height),
        }
    }
}

impl PixelBox {
    // The pixels of `grid` that `rect`, in logical units, may touch; `None`
    // when it touches none.
    fn of(rect: Rect, grid: PixelGrid) -> Option<Self> {
        let PixelGrid {
            scale,
            margin,
            width,
            height,
        } = grid;
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

// The area `rect` takes in the pixels of `grid`, placed at `origin`.
fn area_at(rect: Rect, origin: Point, grid: PixelGrid) -> Area {
    PixelBox::of(rect.translate(origin.x, origin.y), grid).map_or(Area::Hidden, Area::Drawn)
}

// The pixel `value` lies at in a window `most` pixels long, clamped to it.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to 0..=most, and most is at most 65,535"
)]
fn pixel(value: f32, most: f32) -> u16 {
    value.clamp(0.0, most) as u16
}

// How the target drew a record's subtree.
#[derive(Clone, Copy, Debug, Default)]
enum Area {
    // In place, within bounds the walk did not know.
    #[default]
    Unknown,
    Hidden,
    Drawn(PixelBox),
    // Through the record's own surface, with no areas of what lies beneath
    // it: a change there changes all of it.
    Composite(PixelBox),
}

impl Area {
    fn of(shown: ShownArea, grid: PixelGrid) -> Self {
        let pixels = |rect| PixelBox::of(rect, grid);
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
    // Where `within`, in the record's space, lies in the target at the
    // record's origin: drawn or hidden; `None` when the reach is not known
    // or the target did not draw the record in place.
    fn landing(self, within: Reach, grid: PixelGrid) -> Option<Area> {
        match (within, self.origin) {
            (Reach::Empty, _) => Some(Area::Hidden),
            (Reach::Finite(rect), Some(origin)) => Some(area_at(rect, origin, grid)),
            _ => None,
        }
    }

    // The part of the record's own area that `within`, in the record's
    // space, covers, and where `within` lands when it reaches past the own
    // area, as a record whose own draws grow does.
    fn own_part(self, within: Reach, grid: PixelGrid) -> OwnPart {
        let held = |area| OwnPart { area, past: None };
        match (self.landing(within, grid), self.own) {
            (None, own) => held(own),
            (Some(Area::Drawn(part)), Area::Hidden) => OwnPart {
                area: Area::Hidden,
                past: Some(part),
            },
            (Some(Area::Drawn(part)), Area::Drawn(own)) => {
                let kept = part.intersect(own);
                OwnPart {
                    area: kept.map_or(Area::Hidden, Area::Drawn),
                    past: (kept != Some(part)).then_some(part),
                }
            }
            (Some(Area::Drawn(part)), Area::Unknown) => held(Area::Drawn(part)),
            (Some(Area::Drawn(_)), own @ Area::Composite(_)) => held(own),
            (Some(_), _) => held(Area::Hidden),
        }
    }
}

// What a record's own change takes of the target's areas: the part of the
// record's own area it covers, and where it lands when it reaches past
// that area, which the prediction of the frame's damage places anew.
#[derive(Clone, Copy)]
struct OwnPart {
    area: Area,
    past: Option<PixelBox>,
}

// Which areas `TargetState::own_area` reads for a changed record.
#[derive(Clone, Copy)]
enum Moment {
    // The areas a walk kept: the frame before's as a frame begins, its own
    // as it ends.
    Kept,
    // Before the frame's walk, where the change lands at the origins the
    // frame before kept, when it may land past the area the begin took for
    // it: a record's own change at the record's origin, a moved, new or
    // changed record's reach at its parent's.
    Predicted,
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
    region: &'a DamageRects,
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
    // All the record's subtree draws, placed anew: the record moved or is
    // new.
    Placed,
    // What the record draws of its own, within the reach in its space.
    Own(Reach),
}

impl Scope {
    fn of(event: RecordEvent) -> Self {
        match event {
            RecordEvent::Redrawn(within) => Self::Own(within),
            RecordEvent::Moved | RecordEvent::Inserted => Self::Placed,
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
    // The generation of the debug toggles, which any thread may flip.
    pub(crate) debug_toggles: u64,
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
    // Why the frame drew whole for the target's sake.
    pub(crate) own_sake: Option<OwnSake>,
}

#[derive(Default)]
pub(crate) struct TargetState {
    // The version of the records read last.
    read: Option<SceneVersion>,
    // By record row, how the target last drew the record.
    shown: Vec<Shown>,
    setting: Option<TargetSetting>,
    // The frame's damage: from its begin to its end the damage of the old
    // areas, which the end adds the new areas to.
    damage: DamageBuilder,
    // Whether the frame changes the whole window: an area it changes is not
    // known.
    whole: bool,
    // Whether the frame before is not known: the frame draws whole, and that
    // says nothing of how much of the window it changes.
    unknown_before: bool,
    // The records the frame's changes leave and what of each they reach,
    // whose new areas the frame adds after its collect.
    changed: Vec<(RecordRef, Scope)>,
    // By row, the frame that marked the record and how a culled walk of
    // that frame takes it.
    marks: Vec<(u32, Visit)>,
    stamp: u32,
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
    pub(crate) fn begin(&mut self, store: &SceneStore, setting: TargetSetting) {
        self.damage.clear();
        self.changed.clear();
        let read = self.read.replace(store.version());
        self.recording = self.skipped == 0;
        if !self.recording {
            // A frame that changes nothing says so, skipped or not.
            let same = self.setting == Some(setting);
            self.still = same && !store.changed_since(read);
            return;
        }
        self.whole = false;
        self.lost_root = None;
        self.unknown_before = std::mem::take(&mut self.stale) || self.setting != Some(setting);
        self.setting = Some(setting);
        match store.changes_since(read) {
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
        }
    }

    // The changes the records keep for the target from the next update on:
    // a frame that skips working out its damage needs only whether
    // anything changed, which the store tells without entries.
    pub(crate) fn followed(&self) -> RecordEvents {
        if self.recording {
            RecordEvents::ALL
        } else {
            RecordEvents::NONE
        }
    }

    // Between the frame's begin and its end, the damage of the areas the
    // frame before drew the changed records in; `None` when the frame works
    // out no damage or one of them is not known.
    pub(crate) fn begun(&self) -> Option<&DamageRects> {
        let known = self.recording && !self.unknown_before && self.lost_root.is_none();
        (known && !self.whole).then_some(self.damage.as_rects())
    }

    // Writes into `damage` the damage the frame takes when each change whose
    // record may draw past its old area lands as the store and the frame
    // before's origins tell, as `own_area` predicts it. The whole window
    // when the old areas take most of it, or when a changed record's reach
    // is not known and no ancestor holds its area: the walk finds that
    // record's area unknown too. `false` when the frame works out no damage
    // or an old area is not known: the walk then works the areas out.
    pub(crate) fn predict_damage(&self, store: &SceneStore, damage: &mut Damage) -> bool {
        let (Some(setting), Some(begun)) = (self.setting, self.begun()) else {
            return false;
        };
        damage.assign_part(begun);
        damage.settle(setting.viewport);
        let mut whole = false;
        if let Damage::Part(rects) = damage {
            // One run: at the bound of rects, a lone add searches every pair.
            let mut run = rects.run();
            for &change in &self.changed {
                match self.own_area(store, change, Moment::Predicted, setting) {
                    Area::Drawn(pixels) | Area::Composite(pixels) => run.add(pixels.rect()),
                    Area::Hidden => {}
                    Area::Unknown => {
                        whole = true;
                        break;
                    }
                }
            }
        }
        if whole {
            *damage = Damage::Full;
        }
        damage.settle(setting.viewport);
        true
    }

    // What a walk of the frame leaves out of `region` of the records of
    // `store`: it walks every changed record and its ancestors, and all of
    // a moved or new record's subtree.
    pub(crate) fn cull<'a>(&'a mut self, store: &SceneStore, region: &'a DamageRects) -> Cull<'a> {
        self.stamp = self.stamp.wrapping_add(1).max(1);
        for at in 0..self.changed.len() {
            let (record, scope) = self.changed[at];
            let Some(id) = store.resolve(record) else {
                continue;
            };
            let visit = match scope {
                Scope::Subtree | Scope::Placed => Visit::WalkAll,
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
    /// writes the frame's damage into `damage`.
    pub(crate) fn end(
        &mut self,
        store: &SceneStore,
        root: Option<&LayerScene>,
        damage: &mut Damage,
    ) {
        if !self.recording {
            if self.still {
                damage.assign_part(&DamageRects::default());
            } else {
                *damage = Damage::Skipped;
            }
            return;
        }
        self.ended(store, root, damage);
        self.last_root = store.root().map(|root| store.record_ref(root));
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
    /// [`MAX_SKIPPED`] frames, each such frame in a row quadrupling them;
    /// past [`RELEASE_SKIPS`], the areas go until a frame records them
    /// again.
    pub(crate) fn drawn(&mut self, drawn: &TargetDrawn) {
        if drawn.skipped {
            self.skipped = self.skipped.saturating_sub(1);
            return;
        }
        match (&drawn.damage, drawn.own_sake) {
            // Stand-ins go once their pipelines land: a frame drawn whole
            // over them says nothing of what the page changes.
            (Damage::Full, Some(OwnSake::StandIns)) => return,
            (Damage::Full, _) if drawn.knew_before => {}
            (Damage::Full | Damage::Skipped, _) => return,
            (Damage::Part(_), _) => {
                self.full_frames = 0;
                return;
            }
        }
        self.full_frames = self.full_frames.saturating_add(1);
        // A target that held no frame took this one whole for its own sake:
        // the skips start with the second such frame in a row.
        let run = self.full_frames - u32::from(drawn.own_sake == Some(OwnSake::NoFrame));
        if run == 0 {
            return;
        }
        let doublings = 2 * (run - 1);
        self.skipped = 1u32 << doublings.min(MAX_SKIPPED.trailing_zeros());
        self.stale = true;
        if self.skipped >= RELEASE_SKIPS {
            self.shown = Vec::new();
            self.changed = Vec::new();
            self.captures = Vec::new();
        }
    }

    fn ended(&mut self, store: &SceneStore, root: Option<&LayerScene>, damage: &mut Damage) {
        let Some(setting) = self.setting else {
            *damage = Damage::Full;
            return;
        };
        // The end's adds start a run of their own over the old areas: the
        // merges of the frame's damage follow those areas, not the order the
        // begin merged them in.
        self.damage.start_run();
        let shown = root.map_or(&[][..], |root| &root.shown);
        let grid = PixelGrid::of(setting);
        for shown in shown {
            let row = shown.record.id.index();
            if row >= self.shown.len() {
                self.shown.resize(row + 1, Shown::default());
            }
            let area = Area::of(shown.area, grid);
            // Most records draw all they draw themselves: a leaf, and a
            // record drawn through its own surface.
            let own = if shown.own == shown.area {
                area
            } else {
                Area::of(shown.own, grid)
            };
            self.shown[row] = Shown {
                generation: shown.record.generation,
                area,
                own,
                origin: shown.origin,
            };
        }
        // By row arrays doubled to take a record; the room past a quarter
        // of their rows goes back.
        self.shown.trim_growth_slack();
        self.marks.trim_growth_slack();
        if self.unknown_before {
            *damage = Damage::Full;
            return;
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
            *damage = Damage::Full;
            return;
        }
        self.widen_by_captures(root, setting, damage);
    }

    // Widens the frame's damage, as its end worked it out before any
    // capture, by the captures of `root`: a later walk of the frame takes in
    // records the first walk did not, whose backdrops read and write more of
    // the window. A frame whose end took the whole window or skipped its
    // damage keeps that.
    pub(crate) fn widen_again(&mut self, root: &LayerScene, damage: &mut Damage) {
        let Some(setting) = self.setting else {
            return;
        };
        if self.recording && !self.unknown_before && !self.whole {
            self.widen_by_captures(Some(root), setting, damage);
        }
    }

    // Writes into `damage` the frame's damage grown by what the backdrops of
    // `root` read and write where it meets them; the whole window when the
    // frame holds what its captures cannot describe.
    fn widen_by_captures(
        &mut self,
        root: Option<&LayerScene>,
        setting: TargetSetting,
        damage: &mut Damage,
    ) {
        damage.assign_part(self.damage.as_rects());
        if let (Some(root), Damage::Part(rects)) = (root, &mut *damage)
            && !rects.is_empty()
            && root.contains_backdrop()
        {
            self.captures.clear();
            let place = (setting.root_scale, setting.viewport);
            if !collect_captures(root, place, &mut self.captures) {
                *damage = Damage::Full;
                return;
            }
            widen(&self.captures, rects);
        }
        damage.settle(setting.viewport);
    }

    // The area a change of `record` of `scope` takes, as the target's areas
    // say: that of its topmost ancestor drawn hidden or through a surface,
    // or that shows a scene, else its own as the walk kept it. A record gone
    // from the store is found under `parent`.
    fn area_of(
        &self,
        store: &SceneStore,
        (record, scope): (RecordRef, Scope),
        parent: Option<RecordRef>,
    ) -> Area {
        match (self.ancestor_area(store, record, parent), self.setting) {
            (Some(area), _) => area,
            (None, None) => Area::Unknown,
            (None, Some(setting)) => self.own_area(store, (record, scope), Moment::Kept, setting),
        }
    }

    // The area a change of `record` of `scope` takes beside what its
    // ancestors hold, as `moment` reads it in a frame of `setting`.
    fn own_area(
        &self,
        store: &SceneStore,
        (record, scope): (RecordRef, Scope),
        moment: Moment,
        setting: TargetSetting,
    ) -> Area {
        let kept = self.shown(record);
        let grid = PixelGrid::of(setting);
        match (moment, scope) {
            (_, Scope::Own(within)) => kept.map_or(Area::Hidden, |shown| {
                let part = shown.own_part(within, grid);
                match moment {
                    Moment::Kept => part.area,
                    Moment::Predicted => part.past.map_or(Area::Hidden, Area::Drawn),
                }
            }),
            (Moment::Kept, Scope::Subtree | Scope::Placed) => {
                kept.map_or(Area::Hidden, |shown| shown.area)
            }
            // A moved, new or changed record may land anywhere; one whose
            // reach is not known lies in the area of an ancestor that holds
            // it, which the begin took.
            (Moment::Predicted, Scope::Subtree | Scope::Placed) => {
                match self.landed_reach(store, record, grid) {
                    Some(Area::Unknown) if self.ancestor_area(store, record, None).is_some() => {
                        Area::Hidden
                    }
                    landed => landed.unwrap_or(Area::Hidden),
                }
            }
        }
    }

    // The area of the topmost ancestor of `record` that holds the record's
    // area: one drawn hidden or through a surface, or that shows a scene;
    // `None` when the record's own area tells. The walk keeps no area of a
    // record in a scene's space, and a change there changes all the scene
    // draws. A record gone from the store is found under `parent`; one whose
    // parent went too lies in the area of that parent.
    fn ancestor_area(
        &self,
        store: &SceneStore,
        record: RecordRef,
        parent: Option<RecordRef>,
    ) -> Option<Area> {
        let mut current = match store.resolve(record) {
            Some(id) => store.parent(id),
            None => match parent {
                Some(parent) => match store.resolve(parent) {
                    Some(id) => Some(id),
                    None => return Some(Area::Hidden),
                },
                None => None,
            },
        };
        // Up from the parent, each ancestor that holds the area replaces the
        // one below it: the last is the topmost.
        let mut holder = None;
        while let Some(ancestor) = current {
            let area = self
                .shown(store.record_ref(ancestor))
                .map(|shown| shown.area);
            let holds = if scene_of(store.layer(ancestor)).is_some() {
                Some(match area {
                    Some(Area::Drawn(pixels)) => Area::Composite(pixels),
                    Some(area) => area,
                    None => Area::Hidden,
                })
            } else {
                match area {
                    Some(Area::Drawn(_) | Area::Unknown) => None,
                    Some(area) => Some(area),
                    // An ancestor new since the frame: the record moved
                    // under it, and its old parent's change holds where it
                    // was.
                    None => Some(Area::Hidden),
                }
            };
            holder = holds.or(holder);
            current = store.parent(ancestor);
        }
        holder
    }

    // Where the subtree of `record` lands at the origin the frame before
    // drew its parent's space at: its reach there. `None` when that origin
    // is not known, or the record casts an elevation shadow, which the
    // window's light places.
    fn landed_reach(&self, store: &SceneStore, record: RecordRef, grid: PixelGrid) -> Option<Area> {
        let id = store.resolve(record)?;
        let extent = store.layer(id).extent();
        let rect = match (extent.reach, extent.caster) {
            (Reach::Unknown, _) => return Some(Area::Unknown),
            (Reach::Empty, None) => return Some(Area::Hidden),
            (Reach::Finite(rect), None) => rect,
            (_, Some(_)) => return None,
        };
        let origin = store
            .parent(id)
            .and_then(|parent| self.shown(store.record_ref(parent)))
            .and_then(|shown| shown.origin)?;
        Some(area_at(rect, origin, grid))
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
