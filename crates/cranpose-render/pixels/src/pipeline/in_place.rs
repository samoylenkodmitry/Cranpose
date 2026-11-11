//! An update of the scene a picture holds, in place: the changed layers push
//! their subtrees again, their items take the old ones' places, the rest of
//! the scene stays as it was, and the areas to draw again come from the
//! replaced items alone. A subtree that pushes the items it pushed before,
//! in kind and order, writes each over the old one as it pushes it; from an
//! item that differs on, it pushes into a spill scene that is spliced in,
//! and what follows it moves. The update starts from the records the store
//! changed and finds each in the scene by its route, so it reads the changed
//! records and the records above them, and none beside them.

use cranpose_core::{RecordId, RecordRef};
use cranpose_render_common::{
    graph::ProjectiveTransform, graph_scene::RenderDiagnostics, scene_store::SceneStore,
};

use super::{ContentStyle, LightSource, RasterTraversalContext, populate_draws_from_graph};
use crate::{
    damage::merge_areas,
    draw::{ClipBounds, TextRaster},
    rewrite::Rewrite,
    scene::{RasterScene, Subtree},
};

// What the pushes of the changed subtrees share.
struct Repush<'a> {
    scene: &'a mut RasterScene,
    reach: &'a mut Vec<Option<ClipBounds>>,
    // Whether `reach` still holds where each item of `scene` reaches: a
    // splice moves the items after it.
    held: UpdatedReach,
    rewrite: &'a mut Option<Box<Rewrite>>,
    window: ClipBounds,
    areas: &'a mut Vec<ClipBounds>,
    diagnostics: &'a RenderDiagnostics,
    light: LightSource,
    scale: f32,
    text: &'a TextRaster,
}

// The scene cannot take the change in place: it is built again.
struct Rebuild;

// A record whose subtree pushes again, with its place among the scene's
// records and where its subtree's records end, as the update found them.
#[derive(Clone, Copy)]
pub(crate) struct PushRoot {
    place: usize,
    end: usize,
    record: RecordId,
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
            match push_root(scene, store, id, roots) {
                Ok(Some(root)) => roots.push(root),
                Ok(None) => {}
                Err(Rebuild) => return None,
            }
        }
        outermost(roots);
        let held = if reach.len() == scene.order.len() {
            UpdatedReach::Held
        } else {
            UpdatedReach::Lost
        };
        let mut repush = Repush {
            scene,
            reach,
            held,
            rewrite,
            window,
            areas,
            diagnostics,
            light,
            scale,
            text,
        };
        // The subtrees push in the order they draw in, so the areas come in
        // that order.
        let pushed = roots
            .iter()
            .try_for_each(|root| repush.push_root(store, root.record));
        let held = repush.held;
        merge_areas(areas);
        pushed.ok()?;
        Some(held)
    }
}

// Sorts `roots` by their places and drops each that lies in the subtree of
// another: that one's push draws it. The list is short, so it sorts in place
// by insertion.
fn outermost(roots: &mut Vec<PushRoot>) {
    for sorted in 1..roots.len() {
        let mut at = sorted;
        while at > 0 && roots[at - 1].place > roots[at].place {
            roots.swap(at - 1, at);
            at -= 1;
        }
    }
    let (mut kept, mut end) = (0, 0);
    for index in 0..roots.len() {
        let root = roots[index];
        if root.place >= end {
            end = root.end;
            roots[kept] = root;
            kept += 1;
        }
    }
    roots.truncate(kept);
}

// The record whose subtree pushes again for the change of `record`, when one
// does that is not beneath a root of `found`: the record itself, or the
// nearest one above it that the scene holds and that can push.
fn push_root(
    scene: &RasterScene,
    store: &SceneStore,
    record: RecordId,
    found: &[PushRoot],
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
        // A root found for an earlier change pushes the records beneath
        // it, this one among them.
        if found
            .iter()
            .any(|root| (root.place..root.end).contains(&place))
        {
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
        if !asked && !moved && old.changes.same_subtree(now) {
            return Ok(None);
        }
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
        if let Some(parent) = parent
            && passed_down_by(scene, store, parent).is_none()
        {
            id = parent;
            asked = true;
            continue;
        }
        return Ok(Some(PushRoot {
            place,
            end: old.end,
            record: id,
        }));
    }
}

// Where the scene holds what the record `parent` passed down to its child
// layers.
fn passed_down_by(scene: &RasterScene, store: &SceneStore, parent: RecordId) -> Option<usize> {
    scene
        .route(store.record_ref(parent))
        .and_then(|at| scene.subtrees[at].passed_down)
}

impl Repush<'_> {
    // Pushes the subtree of `record` again where the scene holds it now: a
    // push before it may have moved it. Out of line: the push holds the
    // scene's whole walk, and inlined it made the update one function too
    // large for a board to keep in RAM.
    #[inline(never)]
    fn push_root(&mut self, store: &SceneStore, record: RecordId) -> Result<(), Rebuild> {
        self.scene.refresh_routes();
        let place = self.scene.route(store.record_ref(record)).ok_or(Rebuild)?;
        let parent = match store.parent(record) {
            Some(parent) => Some(passed_down_by(self.scene, store, parent).ok_or(Rebuild)?),
            None => None,
        };
        self.push_again(Subtree::of_record(store.layer(record)), place, parent)
    }

    fn push_again(
        &mut self,
        subtree: Subtree<'_>,
        record: usize,
        parent: Option<usize>,
    ) -> Result<(), Rebuild> {
        let passed = parent.and_then(|at| self.scene.passed_down.get(at));
        let transform = passed.map_or_else(
            || ProjectiveTransform::uniform_scale(self.scale),
            |passed| passed.transform,
        );
        let style = passed.map_or(ContentStyle::PLAIN, |passed| ContentStyle {
            alpha: passed.alpha,
            color_filter: passed.color_filter.clone(),
        });
        let context = RasterTraversalContext {
            parent_transform: &transform,
            parent_content_style: &style,
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
        };
        let mut rewrite = self.rewrite.take().unwrap_or_default();
        if !rewrite.start(self.scene, record, self.window, self.reach, self.held) {
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
        let (reach, held, areas) = rewrite.results();
        (*self.reach, self.held) = (reach, held);
        self.areas.append(areas);
        *self.rewrite = Some(rewrite);
        Ok(())
    }
}
