//! An update of the scene a picture holds, in place: the changed layers push
//! their subtrees again, their items take the old ones' places, the rest of
//! the scene stays as it was, and the areas to draw again come from the
//! replaced items alone. A subtree that pushes the items it pushed before,
//! in kind and order, writes each over the old one as it pushes it; from an
//! item that differs on, it pushes into a spill scene that is spliced in,
//! and what follows it moves. The update starts from the records the store
//! changed and finds each in the scene by its route, once, so it reads the
//! changed records and the records above them, and none beside them. All its
//! subtrees push in one rewrite session.

use cranpose_core::{RecordId, RecordRef};
use cranpose_render_common::{
    graph::ProjectiveTransform, graph_scene::RenderDiagnostics, scene_store::SceneStore,
};

use super::{ContentStyle, LightSource, RasterTraversalContext, populate_draws_from_graph};
use crate::{
    damage::merge_areas,
    draw::{ClipBounds, TextRaster},
    rewrite::Rewrite,
    scene::{PassedDown, RasterScene, Subtree},
};

mod frame_shift;

// What the pushes of the changed subtrees share.
struct Repush<'a> {
    scene: &'a mut RasterScene,
    // The update's rewrite session, which holds the reach list and the areas
    // the pushes change. It lies in `scene` while a subtree pushes.
    rewrite: &'a mut Option<Box<Rewrite>>,
    diagnostics: &'a RenderDiagnostics,
    light: LightSource,
    scale: f32,
    text: &'a TextRaster,
}

// The scene cannot take the change in place: it is built again.
struct Rebuild;

// What the update does to the subtree of a changed record.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RootKind {
    // The subtree pushes again.
    Push,
    // The subtree is a frame that only moved its content: it shifts.
    Shift,
}

// A record whose subtree pushes again or shifts, with its place among the
// scene's records, where its subtree's records end and where the scene holds
// what its parent passed down, as the update found them: a splice of an
// earlier subtree that changes how many records or entries the scene holds
// moves them.
#[derive(Clone, Copy)]
pub(crate) struct PushRoot {
    place: usize,
    end: usize,
    parent: Option<usize>,
    record: RecordId,
    kind: RootKind,
}

// A store with the records it changed in its latest `updates` updates, the
// ones a scene lags by.
#[derive(Clone, Copy)]
pub(crate) struct Changed<'a> {
    pub store: &'a SceneStore,
    pub records: &'a [RecordRef],
    pub updates: u32,
}

// Whether the scene's reach list still holds where each item reaches.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum UpdatedReach {
    // It does: the update put the reach of each item it changed in its
    // item's place.
    Held,
    // It does not: a spliced subtree moved the items after it.
    #[default]
    Lost,
}

// A scene to bring up to its store in place, where each of its items
// reaches, and the storage the update borrows: the rewrite that pushes a
// changed subtree again and the list of the subtrees to push.
pub(crate) struct InPlaceUpdate<'a> {
    pub scene: &'a mut RasterScene,
    pub reach: &'a mut Vec<Option<ClipBounds>>,
    pub rewrite: &'a mut Option<Box<Rewrite>>,
    pub roots: &'a mut Vec<PushRoot>,
}

impl InPlaceUpdate<'_> {
    // Pushes again the subtrees of the records `changed` names, the records
    // its store changed since the scene was brought up to it.
    pub(crate) fn run(
        self,
        changed: Changed<'_>,
        diagnostics: &RenderDiagnostics,
        scale: f32,
        text: &TextRaster,
        window: ClipBounds,
        areas: &mut Vec<ClipBounds>,
    ) -> Option<UpdatedReach> {
        let InPlaceUpdate {
            scene,
            reach,
            rewrite,
            roots,
        } = self;
        areas.clear();
        let store = changed.store;
        let light = LightSource::of(store, scale);
        if scene.start != Some(light) {
            return None;
        }
        store.root_layer()?;
        scene.refresh_routes();
        roots.clear();
        for &record in changed.records {
            let Some(id) = store.resolve(record) else {
                continue;
            };
            // A parent that changed itself or moved in those updates pushes
            // its subtree, or leaves it to a record above: the store names
            // the parent too.
            if store
                .parent(id)
                .is_some_and(|parent| store.changed_within(parent, changed.updates))
            {
                continue;
            }
            match push_root(scene, store, id, (roots, changed.records)) {
                Ok(Some(root)) => add_root(roots, root),
                Ok(None) => {}
                Err(Rebuild) => return None,
            }
        }
        let held = if reach.len() == scene.order.len() {
            UpdatedReach::Held
        } else {
            UpdatedReach::Lost
        };
        if roots.is_empty() {
            return Some(held);
        }
        rewrite
            .get_or_insert_default()
            .begin_update(window, reach, held);
        let mut repush = Repush {
            scene,
            rewrite,
            diagnostics,
            light,
            scale,
            text,
        };
        // The subtrees push in the order they draw in, so the areas come in
        // that order.
        let pushed = roots
            .iter()
            .try_for_each(|root| repush.push_root(store, root));
        let held = session(repush.rewrite).end_update(reach, areas);
        merge_areas(areas);
        pushed.ok()?;
        Some(held)
    }
}

// Whether a root of `found` pushes the record at `place`: the roots lie
// apart, in the order of their places. A store mostly names its records in
// that order, so the last root answers for most.
#[inline]
fn pushed_by(found: &[PushRoot], place: usize) -> bool {
    match found.last() {
        None => false,
        Some(last) if last.place <= place => place < last.end,
        Some(_) => pushed_by_earlier(found, place),
    }
}

#[cold]
#[inline(never)]
fn pushed_by_earlier(found: &[PushRoot], place: usize) -> bool {
    let after = found.partition_point(|root| root.place <= place);
    after
        .checked_sub(1)
        .and_then(|before| found.get(before))
        .is_some_and(|root| place < root.end)
}

// Adds `root` to `found` where its place puts it, in place of the roots
// beneath it: its push draws them.
#[inline]
fn add_root(found: &mut Vec<PushRoot>, root: PushRoot) {
    if found.last().is_none_or(|last| last.end <= root.place) {
        found.push(root);
    } else {
        add_root_between(found, root);
    }
}

#[cold]
#[inline(never)]
fn add_root_between(found: &mut Vec<PushRoot>, root: PushRoot) {
    let at = found.partition_point(|other| other.place < root.place);
    let beneath = found
        .iter()
        .skip(at)
        .take_while(|other| other.place < root.end)
        .count();
    found.drain(at..at + beneath);
    found.insert(at, root);
}

// The record whose subtree pushes again or shifts for the change of
// `record`, when one does that is not beneath a root of `found`: the record
// itself, or the nearest one above it that the scene holds and that can
// push. `stamps` are the records the store changed.
fn push_root(
    scene: &RasterScene,
    store: &SceneStore,
    record: RecordId,
    (found, stamps): (&[PushRoot], &[RecordRef]),
) -> Result<Option<PushRoot>, Rebuild> {
    let mut id = record;
    // Whether a record beneath `id` leaves its push to it, whatever `id`
    // changed itself.
    let mut asked = false;
    loop {
        let parent = store.parent(id);
        // The scene holds no record of it: the record above draws it.
        let Some(place) = scene.route(store.record_ref(id)) else {
            id = parent.ok_or(Rebuild)?;
            asked = true;
            continue;
        };
        // A subtree beneath a frame lies where the frame was pushed: the
        // frame pushes again in its place.
        if let Some(frame) = frame_shift::frame_above(scene, store, place) {
            id = frame;
            asked = true;
            continue;
        }
        // A root found for an earlier change pushes the records beneath
        // it, this one among them.
        if pushed_by(found, place) {
            return Ok(None);
        }
        let old = &scene.subtrees[place];
        let subtree = Subtree::of_record(store.layer(id));
        // The record holds another layer: the record above pushes.
        if old.identity != subtree.identity() {
            id = parent.ok_or(Rebuild)?;
            asked = true;
            continue;
        }
        let now = subtree.changes();
        let moved = !old.changes.same_place(now);
        // A layer that changed nothing of its own, with records of its
        // children, leaves the change to the records beneath it, which the
        // store names too. A node drawn inline has none beneath its own.
        let pushes = asked
            || moved
            || match subtree {
                Subtree::Inline(_) => true,
                Subtree::Layer(_) => {
                    !old.changes.same_own(now) || old.end == place + 1 || old.passed_down.is_none()
                }
            };
        if !pushes {
            return Ok(None);
        }
        // A parent that passed nothing down pushes its own subtree.
        let passed = match passed_down_above(scene, store, parent) {
            Ok(passed) => passed,
            Err(parent) => {
                id = parent;
                asked = true;
                continue;
            }
        };
        let shifts = !asked
            && moved
            && matches!(subtree, Subtree::Layer(_))
            && frame_shift::can_shift(scene, store, (old, id), stamps);
        return Ok(Some(PushRoot {
            place,
            end: old.end,
            parent: passed,
            record: id,
            kind: if shifts {
                RootKind::Shift
            } else {
                RootKind::Push
            },
        }));
    }
}

// Where the scene holds what the record above a root, `parent`, passed down:
// `None` for a root with no record above it, and the record itself when it
// passed nothing down.
fn passed_down_above(
    scene: &RasterScene,
    store: &SceneStore,
    parent: Option<RecordId>,
) -> Result<Option<usize>, RecordId> {
    let Some(parent) = parent else {
        return Ok(None);
    };
    passed_down_by(scene, store, parent).map(Some).ok_or(parent)
}

// Where the scene holds what the record `parent` passed down to its child
// layers.
fn passed_down_by(scene: &RasterScene, store: &SceneStore, parent: RecordId) -> Option<usize> {
    scene
        .route(store.record_ref(parent))
        .and_then(|at| scene.subtrees[at].passed_down)
}

// The update's rewrite session in the slot that holds it while no subtree
// pushes.
fn session(slot: &mut Option<Box<Rewrite>>) -> &mut Rewrite {
    slot.as_deref_mut()
        .expect("the update holds its rewrite between pushes")
}

impl<'a> Repush<'a> {
    // Brings the subtree of `root` up to the store where the scene holds it
    // now: the places found for it hold, unless a splice of an earlier
    // subtree moved them. A subtree that only moved its content as a frame
    // shifts, and any other pushes again. Out of line: the push holds the
    // scene's whole walk, and inlined it made the update one function too
    // large for a board to keep in RAM.
    #[inline(never)]
    fn push_root(&mut self, store: &SceneStore, root: &PushRoot) -> Result<(), Rebuild> {
        let record = root.record;
        let (place, parent) = if session(self.rewrite).shifted() {
            self.find_again(store, record)?
        } else {
            (root.place, root.parent)
        };
        if root.kind == RootKind::Shift && self.shift_frame(store, record, place, parent) {
            return Ok(());
        }
        if !self.scene.frames.is_empty() {
            self.forget_frames_within(place);
        }
        self.push_again(Subtree::of_record(store.layer(record)), place, parent)
    }

    // Where the scene holds the record now and what its parent passed down,
    // after a splice moved them.
    #[cold]
    #[inline(never)]
    fn find_again(
        &mut self,
        store: &SceneStore,
        record: RecordId,
    ) -> Result<(usize, Option<usize>), Rebuild> {
        self.scene.refresh_routes();
        let place = self.scene.route(store.record_ref(record)).ok_or(Rebuild)?;
        let parent = match store.parent(record) {
            Some(parent) => Some(passed_down_by(self.scene, store, parent).ok_or(Rebuild)?),
            None => None,
        };
        Ok((place, parent))
    }

    // What the children of the record that passed down `passed` inherit: the
    // window's, for none.
    #[expect(
        clippy::inline_always,
        reason = "the push and the frame step call it: out of line, the dashboard update took more instructions"
    )]
    #[inline(always)]
    fn inherited(&self, passed: Option<&PassedDown>) -> (ProjectiveTransform, ContentStyle) {
        let transform = passed.map_or_else(
            || ProjectiveTransform::uniform_scale(self.scale),
            |passed| passed.transform,
        );
        let style = passed.map_or(ContentStyle::PLAIN, |passed| ContentStyle {
            alpha: passed.alpha,
            color_filter: passed.color_filter.clone(),
        });
        (transform, style)
    }

    // The context a push of the children of the record that passed down
    // `passed` runs in, with what they `inherit`.
    #[expect(
        clippy::inline_always,
        reason = "the push and the frame step call it: out of line, the dashboard update took more instructions"
    )]
    #[inline(always)]
    fn context<'c>(
        &self,
        passed: Option<&PassedDown>,
        inherited: &'c (ProjectiveTransform, ContentStyle),
    ) -> RasterTraversalContext<'c>
    where
        'a: 'c,
    {
        RasterTraversalContext {
            parent_transform: &inherited.0,
            parent_content_style: &inherited.1,
            parent_visual_clip: passed.and_then(|passed| passed.clip),
            inherited_translated_snap_anchor: passed.and_then(|passed| passed.snap_anchor),
            inherited_translated_content_context: passed.is_some_and(|passed| passed.translated),
            diagnostics: self.diagnostics,
            light: self.light,
            same_as_before: false,
            text: self.text,
            surface_root: false,
            // A scene draws its surfaces out of the graph's order, so the
            // walk to a changed layer stops at a scene's first child in
            // 3D: the layers it pushes again lie outside every space.
            in_space: false,
        }
    }

    fn push_again(
        &mut self,
        subtree: Subtree<'_>,
        record: usize,
        parent: Option<usize>,
    ) -> Result<(), Rebuild> {
        let passed = parent.and_then(|at| self.scene.passed_down.get(at));
        let inherited = self.inherited(passed);
        let context = self.context(passed, &inherited);
        let mut rewrite = self.rewrite.take().expect("the update holds its rewrite");
        if !rewrite.start(self.scene, record) {
            *self.rewrite = Some(rewrite);
            return Err(Rebuild);
        }
        self.scene.rewrite = Some(rewrite);
        populate_draws_from_graph(subtree, self.scene, &context, &mut None);
        let mut rewrite = self
            .scene
            .rewrite
            .take()
            .expect("the rewrite started above");
        rewrite.finish(self.scene, record);
        // The records the subtree pushed lie where its old ones did: the
        // routes of those that took another's place are noted, unless all
        // routes wait to be filled again.
        if rewrite.rerouted() && !self.scene.routes_stale {
            let end = self.scene.subtrees.get(record).map_or(record, |at| at.end);
            self.scene.route_records(record..end);
        }
        *self.rewrite = Some(rewrite);
        Ok(())
    }
}
