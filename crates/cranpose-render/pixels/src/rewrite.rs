//! A changed subtree pushed again over its old items, records and
//! passed-down entries in the scene that holds them. A record takes the
//! place of the old record of the same node where its old items start; a
//! node's items take the places of its own old items while they match them
//! in kind and draw order, and the pair adds the area where the two draw
//! differently; a child subtree the walk keeps stays in its place. From the
//! first of these that does not fit, the subtree pushes into a spill scene,
//! which takes copies of the kept subtrees. The spill's items take the place
//! of the old ones left before the old record of the next child the walk
//! finds, or when the subtree ends, and what follows moves. A frame mostly
//! pushes a changed subtree as it pushed it before, so the spill seldom
//! holds anything, and a child that comes or goes spills no more than its
//! own subtree.

use cranpose_render_common::graph::LayerNode;

use crate::{
    damage::{ItemReach, add_changed_pair, add_changed_runs, merge_areas},
    draw::ClipBounds,
    layout_walk::child_record_from,
    pipeline::UpdatedReach,
    raster_scene::{HitLayer, ItemCounts, PassedDown, RasterScene, Subtree, SubtreeItems},
    scene_item::SceneItem,
};

// Where in a scene's lists a subtree's next item, record and passed-down
// entry go.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Places {
    items: ItemCounts,
    records: usize,
    passed_down: usize,
}

// Whether a finished rewrite spliced a run of the scene's lists: the records
// and entries after it may then lie at other indices.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Replaced {
    #[default]
    Nothing,
    Run,
}

// The room for changed areas a rewrite keeps from one to the next.
const KEPT_AREAS: usize = 32;

// A record the rewrite put in place of the old record of its node, and
// what the old record held: where its subtree's records and draw order
// ended, and its passed-down entry.
#[derive(Clone, Copy)]
struct Opened {
    end: usize,
    stop: usize,
    passed_down: Option<usize>,
}

#[derive(Default)]
pub(crate) struct Rewrite {
    // While the rewrite pushes in place, `next.items` holds the old scene's
    // counts before one of its items, as each old record's start does: two
    // such counts with the same draw order are the same.
    next: Places,
    // Where the old subtree's items, records and entries end.
    stop: Places,
    // Where the old items the open record may take the places of end: its
    // own, before its first child subtree, or before the record's old end
    // after the last.
    own_items_end: usize,
    // The records opened in place and not closed yet, the last innermost.
    opened: Vec<Opened>,
    // Whether the rewrite replaced or let go a record that hit targets read.
    hits_replaced: bool,
    // Where the subtree's pushes went on in `spill` from, once one did not
    // match the old item in its place.
    spilled_at: Option<Places>,
    spill: RasterScene,
    // The record of the subtree.
    record: usize,
    // How many records the splices so far added, less those they removed:
    // an old record after them lies that many places on.
    records_moved: isize,
    // Whether a splice replaced a run of the scene's lists.
    replaced: Replaced,
    // The part of the frame whose changed pixels `areas` takes.
    window: ClipBounds,
    // Whether `reach` holds where each item of the scene reaches.
    held: UpdatedReach,
    reach: Vec<Option<ClipBounds>>,
    areas: Vec<ClipBounds>,
}

impl Rewrite {
    // Starts the rewrite of the subtree whose record in `scene` is
    // `record`, whose changed pixels in `window` it will add to its areas,
    // taking `reach`, which holds where each item reaches when `held` says
    // so.
    // `false`, with `reach` left, when the subtree's passed-down entries do
    // not lie in one run.
    pub(crate) fn start(
        &mut self,
        scene: &RasterScene,
        record: usize,
        window: ClipBounds,
        reach: &mut Vec<Option<ClipBounds>>,
        held: UpdatedReach,
    ) -> bool {
        let Some((extent, first)) = scene.subtree_span(record) else {
            return false;
        };
        let head = &scene.subtrees[record];
        // A subtree with no entries of its own takes its new ones after the
        // entries of the records before it, which the walk pushed first: the
        // records and their entries lie in the order of the walk, so the
        // last record before it with an entry holds the last of them.
        let first = if extent.passed_down == 0 {
            scene.subtrees[..record]
                .iter()
                .rev()
                .find_map(|before| before.passed_down)
                .map_or(0, |last| last + 1)
        } else {
            first
        };
        self.next = Places {
            items: head.start,
            records: record,
            passed_down: first,
        };
        self.stop = Places {
            items: head.stop,
            records: record + extent.records,
            passed_down: first + extent.passed_down,
        };
        self.own_items_end = self.next.items.order;
        self.opened.clear();
        self.hits_replaced = false;
        self.spilled_at = None;
        self.record = record;
        self.records_moved = 0;
        self.replaced = Replaced::Nothing;
        debug_assert!(
            self.spill.order.is_empty() && self.spill.subtrees.is_empty(),
            "a rewrite leaves its spill empty when it finishes"
        );
        self.window = window;
        (self.reach, self.held) = (std::mem::take(reach), held);
        true
    }

    // How many items of each kind the scene would hold up to where the
    // subtree pushes next, were the spill in place.
    pub(crate) fn counts(&self) -> ItemCounts {
        self.here().items
    }

    // How many records and items of each kind the scene would hold up to
    // where the subtree opens its next record, were the spill in place: the
    // end of a record that closes there.
    pub(crate) fn record_end(&self) -> (usize, ItemCounts) {
        let here = self.here();
        (here.records, here.items)
    }

    // Where the subtree pushes its next item, opens its next record and
    // notes its next passed-down entry, were the spill in place.
    #[inline]
    fn here(&self) -> Places {
        match self.spilled_at {
            Some(at) => Places {
                items: self.spill.counts().moved(ItemCounts::default(), at.items),
                records: at.records + self.spill.subtrees.len(),
                passed_down: at.passed_down + self.spill.passed_down.len(),
            },
            None => self.next,
        }
    }

    // Pushes `item` in place of the old item where the subtree pushes next,
    // when that one is of its kind, adding the areas the two draw
    // differently; else, and from then on, into the spill.
    pub(crate) fn place<T: SceneItem>(&mut self, scene: &mut RasterScene, item: T) {
        if self.spilled_at.is_none() {
            let at = self.next.items.order;
            let slot = *T::count(&mut self.next.items);
            let (list, order) = T::lists(scene);
            if at < self.own_items_end && T::index_of(order[at]) == Some(slot) {
                let reach = match self.reach.get_mut(at) {
                    Some(held) if self.held == UpdatedReach::Held => ItemReach::Replaced(held),
                    _ => ItemReach::Unknown,
                };
                add_changed_pair(
                    list[slot].as_ref(),
                    item.as_ref(),
                    reach,
                    self.window,
                    &mut self.areas,
                );
                list[slot] = item;
                self.next.items.order += 1;
                *T::count(&mut self.next.items) += 1;
                return;
            }
            self.spilled_at = Some(self.next);
        }
        self.spill.push_item(item);
    }

    // Writes the record of `subtree`, with `hits`, its `hit_layer`, over the
    // old record where the subtree opens its next one, when that one is of
    // the same node and its items start where the record's do; else into
    // the spill. Returns where the record would lie, were the spill in
    // place. The old record's box of hit facts takes the new ones in place.
    pub(crate) fn place_record(
        &mut self,
        records: &mut [SubtreeItems],
        (subtree, hits): (Subtree<'_>, Option<&LayerNode>),
    ) -> usize {
        if self.spilled_at.is_none() {
            let index = self.next.records;
            if let Some(old) = records[..self.stop.records].get(index)
                && old.identity == subtree.identity()
                && old.start.order == self.next.items.order
            {
                self.opened.push(Opened {
                    end: old.end,
                    stop: old.stop.order,
                    passed_down: old.passed_down,
                });
                self.own_items_end = own_items_end(records, index, old.end, old.stop.order);
                let record = &mut records[index];
                if record.hits.is_some() || hits.is_some() {
                    let kept = record.hits.take();
                    self.hits_replaced |= kept.is_some();
                    record.hits = boxed_hits(kept, hits);
                }
                record.transform = subtree.transform_to_parent();
                record.node_size = subtree.node_size();
                record.passed_down = None;
                record.end = index + 1;
                record.stop = self.next.items;
                self.next.records += 1;
                return index;
            }
            self.spilled_at = Some(self.next);
        }
        let here = self.here();
        self.spill
            .subtrees
            .push(subtree.record(here.records + 1, here.items, hits));
        here.records
    }

    // The record that lies at `index` were the spill in place.
    pub(crate) fn record_mut<'a>(
        &'a mut self,
        records: &'a mut [SubtreeItems],
        index: usize,
    ) -> Option<&'a mut SubtreeItems> {
        match self.spilled_at {
            Some(at) if index >= at.records => self.spill.subtrees.get_mut(index - at.records),
            _ => records.get_mut(index),
        }
    }

    // Puts `entry`, the passed-down entry of the open record, in place of
    // the old record's, when it had one where the subtree notes its next
    // one; else into the spill. Returns where the entry would lie, were the
    // spill in place, and whether it is what the old record passed down.
    pub(crate) fn place_passed_down(
        &mut self,
        entries: &mut [PassedDown],
        entry: PassedDown,
    ) -> (usize, bool) {
        let old = self.opened.last().and_then(|opened| opened.passed_down);
        if self.spilled_at.is_none() {
            let index = self.next.passed_down;
            if old == Some(index) {
                let same = entries[index] == entry;
                entries[index] = entry;
                self.next.passed_down += 1;
                return (index, same);
            }
            self.spilled_at = Some(self.next);
        }
        let index = self.here().passed_down;
        self.spill.passed_down.push(entry);
        (index, false)
    }

    // Notes that the innermost open record closed: the items its parent
    // may take the places of go on after the closed record's old subtree,
    // as far as the parent's next child subtree.
    pub(crate) fn close_record(&mut self, records: &[SubtreeItems]) {
        if self.spilled_at.is_some() {
            return;
        }
        if let Some(closed) = self.opened.pop() {
            self.after_old_subtree(records, closed.end);
        }
    }

    // Opens again, in place, the old record `at` of a node whose record, own
    // items and passed-down entry stay as they were, and keeps its own items
    // up to `first_child`, the record of its first child subtree. Returns
    // the entry. `None`, with nothing changed, when the record does not lie
    // where the subtree pushes next or its entry does not come next.
    pub(crate) fn reopen(
        &mut self,
        records: &[SubtreeItems],
        at: usize,
        first_child: usize,
    ) -> Option<usize> {
        let old = records[..self.stop.records].get(at)?;
        let entry = old.passed_down?;
        let child = records[..old.end].get(first_child)?;
        if self.spilled_at.is_some()
            || self.next.records != at
            || self.next.items.order != old.start.order
            || self.next.passed_down != entry
        {
            return None;
        }
        self.opened.push(Opened {
            end: old.end,
            stop: old.stop.order,
            passed_down: old.passed_down,
        });
        self.next = Places {
            items: child.start,
            records: first_child,
            passed_down: entry + 1,
        };
        self.own_items_end = child.start.order;
        Some(entry)
    }

    // Keeps the old subtree of the record `old`, a child of the open
    // record: in its place when it lies where the subtree pushes next,
    // else as a copy in the spill.
    pub(crate) fn keep(&mut self, scene: &RasterScene, old: usize) {
        let records = &scene.subtrees;
        let head = &records[old];
        let Some((extent, first_entry)) = scene.subtree_span(old) else {
            return;
        };
        let entries_fit = extent.passed_down == 0 || first_entry == self.next.passed_down;
        if self.spilled_at.is_none()
            && self.next.records == old
            && self.next.items.order == head.start.order
            && entries_fit
        {
            self.next = Places {
                items: head.stop,
                records: head.end,
                passed_down: self.next.passed_down + extent.passed_down,
            };
            self.after_old_subtree(records, head.end);
            return;
        }
        if self.spilled_at.is_none() {
            self.spilled_at = Some(self.next);
        }
        let here = self.here();
        let to = (here.items, here.records, here.passed_down);
        self.spill
            .copy_subtrees_to(scene, old..head.end, (head.stop, first_entry), to);
    }

    // Sets where the open record's own old items go on after the old
    // subtree that ended at the record `end`: up to its next child subtree,
    // or to its old end after the last one.
    fn after_old_subtree(&mut self, records: &[SubtreeItems], end: usize) {
        let Some(open) = self.opened.last() else {
            self.own_items_end = self.stop.items.order;
            return;
        };
        self.own_items_end = match records[..open.end].get(end) {
            Some(next) if next.identity != (None, None) => next.start.order,
            _ => open.stop,
        };
    }

    // How many records the splices of the rewrite so far added, less those
    // they removed.
    pub(crate) fn records_moved(&self) -> isize {
        self.records_moved
    }

    // Readies the pushes of the next child, whose old record is `at`, to take
    // the places of its old ones: what the subtree pushed since its pushes
    // last fit their places takes the place of the old records, items and
    // entries left before `at`, those of a child that went away included.
    // Returns where the old record now lies.
    pub(crate) fn rejoin(&mut self, scene: &mut RasterScene, at: usize) -> usize {
        let from = match self.spilled_at {
            Some(from) => from,
            None if self.next.records == at => return at,
            None => self.next,
        };
        self.splice_before(scene, from, at)
    }

    // What `rejoin` does for a child whose old record does not lie where the
    // subtree pushes next. Out of line, as `replace` is.
    #[cold]
    #[inline(never)]
    fn splice_before(&mut self, scene: &mut RasterScene, from: Places, at: usize) -> usize {
        let entries = scene.subtrees[from.records..at]
            .iter()
            .filter(|record| record.passed_down.is_some())
            .count();
        let to = Places {
            items: scene.subtrees[at].start,
            records: at,
            passed_down: from.passed_down + entries,
        };
        self.next = self.here();
        self.replace(scene, from, to);
        self.spilled_at = None;
        self.own_items_end = self.next.items.order;
        self.next.records
    }

    // Ends the rewrite. When its pushes went on in the spill, or stopped
    // before the old items' end, what it pushed from there takes the place
    // of the old items, records and entries left, and the records that hold
    // the subtree or follow it move by the difference. Says whether a splice
    // replaced a run.
    pub(crate) fn finish(&mut self, scene: &mut RasterScene) -> Replaced {
        let from = match self.spilled_at {
            Some(at) => at,
            None if self.next == self.stop => return self.replaced,
            None => self.next,
        };
        self.replace(scene, from, self.stop);
        self.opened.clear();
        Replaced::Run
    }

    // Puts what the subtree pushed into the spill in place of the old items,
    // records and entries from `from` up to `to`. The records that hold the
    // subtree, those that follow the run, the open records' old ends and the
    // old subtree's end move by the difference. Out of line: most rewrites
    // push the items they pushed before. The splices would take room in the
    // instruction cache from the code of those rewrites.
    #[cold]
    #[inline(never)]
    fn replace(&mut self, scene: &mut RasterScene, from: Places, to: Places) {
        let order = from.items.order..to.items.order;
        let reach = (self.reach.get(order.clone())).filter(|_| self.held == UpdatedReach::Held);
        add_changed_runs(
            (scene, &scene.order[order]),
            (&self.spill, &self.spill.order),
            reach,
            self.window,
            &mut self.areas,
        );
        self.hits_replaced |= scene.subtrees[from.records..to.records]
            .iter()
            .any(|record| record.hits.is_some());
        let removed = to.items.since(from.items);
        let added = self.spill.counts();
        let records = (to.records - from.records, self.spill.subtrees.len());
        let entries = (
            to.passed_down - from.passed_down,
            self.spill.passed_down.len(),
        );
        // The reach list holds where each spilled item reaches in place of
        // the old run's, so it still holds after the splice.
        if self.held == UpdatedReach::Held {
            let (spill, window) = (&self.spill, self.window);
            let spilled = spill
                .order
                .iter()
                .map(|&item| spill.item_bounds(item, window));
            self.reach.splice(from.items.order..to.items.order, spilled);
        }
        splice_items(scene, (from.items, to.items), &mut self.spill);
        scene
            .subtrees
            .splice(from.records..to.records, self.spill.subtrees.drain(..));
        scene.passed_down.splice(
            from.passed_down..to.passed_down,
            self.spill.passed_down.drain(..),
        );
        let shift = |index: usize, (removed, added): (usize, usize)| index - removed + added;
        let record = self.record;
        for holder in &mut scene.subtrees[..record] {
            if holder.end > record {
                holder.end = shift(holder.end, records);
                holder.stop = holder.stop.shifted(removed, added);
            }
        }
        for later in &mut scene.subtrees[from.records + records.1..] {
            later.end = shift(later.end, records);
            later.start = later.start.shifted(removed, added);
            later.stop = later.stop.shifted(removed, added);
            later.passed_down = later.passed_down.map(|index| shift(index, entries));
        }
        for open in &mut self.opened {
            open.end = shift(open.end, records);
            open.stop = shift(open.stop, (removed.order, added.order));
        }
        self.stop = Places {
            items: self.stop.items.shifted(removed, added),
            records: shift(self.stop.records, records),
            passed_down: shift(self.stop.passed_down, entries),
        };
        self.records_moved += records.1 as isize - records.0 as isize;
        // The runs copied in place from the scene the walk started from no
        // longer lie where they did.
        scene.kept.clear();
        self.replaced = Replaced::Run;
        self.spill.clear();
    }

    // Hands back the reach list the rewrite took and whether it still holds
    // where each item of the scene reaches. Puts the areas it changed,
    // merged and in reverse order, after those `areas` holds. The room that
    // a rewrite of many items took for their areas goes. `areas` takes no
    // more than the merged ones: a canvas drawn again each frame holds what
    // one drawn once does.
    pub(crate) fn results(
        &mut self,
        areas: &mut Vec<ClipBounds>,
    ) -> (Vec<Option<ClipBounds>>, UpdatedReach) {
        merge_areas(&mut self.areas);
        let from = areas.len();
        areas.append(&mut self.areas);
        areas[from..].reverse();
        if self.areas.capacity() > KEPT_AREAS {
            self.areas = Vec::new();
        }
        (std::mem::take(&mut self.reach), self.held)
    }

    // Whether the rewrite replaced or let go a record that hit targets read.
    pub(crate) fn replaced_hits(&self) -> bool {
        self.hits_replaced
    }

    // Lets the spill's storage go.
    pub(crate) fn release_spill(&mut self) {
        self.spill = RasterScene::new();
    }

    // The storage of the spill, for a scene built anew.
    pub(crate) fn take_storage(&mut self) -> RasterScene {
        std::mem::take(&mut self.spill)
    }

    // Takes `scene`, emptied, as the spill's storage.
    pub(crate) fn give_storage(&mut self, scene: RasterScene) {
        self.spill = scene;
    }
}

// What the hit targets beneath `layer` take from it, in `kept`, the box of
// an old record, when there is one.
fn boxed_hits(kept: Option<Box<HitLayer>>, layer: Option<&LayerNode>) -> Option<Box<HitLayer>> {
    match (kept, layer) {
        (Some(mut kept), Some(layer)) => {
            kept.take_from(layer);
            Some(kept)
        }
        (_, layer) => layer.map(|layer| Box::new(HitLayer::of(layer))),
    }
}

// Where the items that the record `index`, whose old subtree ended at the
// record `end` and at `stop` in the draw order, may take the places of end:
// before its first child subtree, or at `stop` when it has none. A draw group of the
// node's own draws is no child subtree.
fn own_items_end(records: &[SubtreeItems], index: usize, end: usize, stop: usize) -> usize {
    let child = child_record_from(records, index + 1, end);
    records[..end]
        .get(child)
        .map_or(stop, |record| record.start.order)
}

// The items of the run `to..old_stop` of `scene` give way to all of
// `from`'s, in each list and in the draw order, and the order's items after
// the run point where their items now lie.
fn splice_items(
    scene: &mut RasterScene,
    (to, old_stop): (ItemCounts, ItemCounts),
    from: &mut RasterScene,
) {
    let (removed, added) = (old_stop.since(to), from.counts());
    scene
        .shapes
        .splice(to.shapes..old_stop.shapes, from.shapes.drain(..));
    scene
        .images
        .splice(to.images..old_stop.images, from.images.drain(..));
    scene
        .texts
        .splice(to.texts..old_stop.texts, from.texts.drain(..));
    scene
        .shadows
        .splice(to.shadows..old_stop.shadows, from.shadows.drain(..));
    scene.order.splice(
        to.order..old_stop.order,
        from.order
            .drain(..)
            .map(|item| item.moved(ItemCounts::default(), to)),
    );
    // The items after the run lie after its items in each list.
    for item in &mut scene.order[to.order + added.order..] {
        *item = item.moved(removed, added);
    }
}
