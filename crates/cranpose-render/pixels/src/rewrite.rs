//! A changed subtree pushed again over its old items, records and
//! passed-down entries in the scene that holds them. While the new items
//! match the old ones in kind and draw order, each takes the old item's
//! place, and the pair adds the area where the two draw differently. From
//! the first one that does not match, the subtree pushes into a spill
//! scene, whose items take the place of the old ones left when the subtree
//! ends, and what follows moves. A frame mostly pushes a changed subtree as
//! it pushed it before, so the spill seldom holds anything.

use crate::{
    damage::{ItemReach, add_changed_pair, add_changed_runs},
    draw::{ClipBounds, RenderItem},
    pipeline::UpdatedReach,
    scene::{ItemCounts, PassedDown, RasterScene, SubtreeItems},
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

#[derive(Default)]
pub(crate) struct Rewrite {
    next: Places,
    // Where the old subtree's items, records and entries end.
    stop: Places,
    // Where the subtree's pushes went on in `spill` from, once one did not
    // match the old item in its place.
    spilled_at: Option<Places>,
    spill: RasterScene,
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
        // entries of the records before it, which the walk pushed first.
        let first = if extent.passed_down == 0 {
            scene.subtrees[..record]
                .iter()
                .filter_map(|before| before.passed_down)
                .max()
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
        self.spilled_at = None;
        self.spill.clear();
        self.window = window;
        (self.reach, self.held) = (std::mem::take(reach), held);
        true
    }

    // How many items of each kind the scene would hold up to where the
    // subtree pushes next, were the spill in place.
    pub(crate) fn counts(&self) -> ItemCounts {
        match self.spilled_at {
            Some(at) => self.spill.counts().moved(ItemCounts::default(), at.items),
            None => self.next.items,
        }
    }

    // How many records the scene would hold up to where the subtree opens
    // its next one, were the spill in place.
    pub(crate) fn record_count(&self) -> usize {
        match self.spilled_at {
            Some(at) => at.records + self.spill.subtrees.len(),
            None => self.next.records,
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
            if at < self.stop.items.order && T::index_of(order[at]) == Some(slot) {
                let old = std::mem::replace(&mut list[slot], item);
                let reach = match self.reach.get_mut(at) {
                    Some(held) if self.held == UpdatedReach::Held => ItemReach::Replaced(held),
                    _ => ItemReach::Unknown,
                };
                add_changed_pair(
                    old.as_ref(),
                    list[slot].as_ref(),
                    reach,
                    self.window,
                    &mut self.areas,
                );
                self.next.items.order += 1;
                *T::count(&mut self.next.items) += 1;
                return;
            }
            self.spilled_at = Some(self.next);
        }
        self.spill.push_item(item);
    }

    // Puts `record` in place of the old record where the subtree opens its
    // next one, while the old subtree has one there; else into the spill.
    // Returns where the record would lie, were the spill in place.
    pub(crate) fn place_record(
        &mut self,
        records: &mut [SubtreeItems],
        record: SubtreeItems,
    ) -> usize {
        if self.spilled_at.is_none() {
            let index = self.next.records;
            if index < self.stop.records {
                records[index] = SubtreeItems {
                    end: index + 1,
                    ..record
                };
                self.next.records += 1;
                return index;
            }
            self.spilled_at = Some(self.next);
        }
        let index = self.record_count();
        self.spill.subtrees.push(SubtreeItems {
            end: index + 1,
            ..record
        });
        index
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

    // Puts `entry` in place of the old passed-down entry where the subtree
    // notes its next one, while the old subtree has one there; else into
    // the spill. Returns where the entry would lie, were the spill in place.
    pub(crate) fn place_passed_down(
        &mut self,
        entries: &mut [PassedDown],
        entry: PassedDown,
    ) -> usize {
        if self.spilled_at.is_none() {
            let index = self.next.passed_down;
            if index < self.stop.passed_down {
                entries[index] = entry;
                self.next.passed_down += 1;
                return index;
            }
            self.spilled_at = Some(self.next);
        }
        let index = self.passed_down_count();
        self.spill.passed_down.push(entry);
        index
    }

    // How many passed-down entries the scene would hold up to where the
    // subtree notes its next one, were the spill in place.
    fn passed_down_count(&self) -> usize {
        match self.spilled_at {
            Some(at) => at.passed_down + self.spill.passed_down.len(),
            None => self.next.passed_down,
        }
    }

    // Ends the rewrite of the subtree whose record in `scene` is `record`.
    // When its pushes went on in the spill, or stopped before the old
    // items' end, what it pushed from there takes the place of the old
    // items, records and entries left, and the records that hold the
    // subtree or follow it move by the difference.
    pub(crate) fn finish(&mut self, scene: &mut RasterScene, record: usize) {
        let from = match self.spilled_at {
            Some(at) => at,
            None if self.next == self.stop => return,
            None => self.next,
        };
        let stop = self.stop;
        let order = from.items.order..stop.items.order;
        let reach = (self.reach.get(order.clone())).filter(|_| self.held == UpdatedReach::Held);
        add_changed_runs(
            (scene, &scene.order[order]),
            (&self.spill, &self.spill.order),
            reach,
            self.window,
            &mut self.areas,
        );
        let removed = stop.items.since(from.items);
        let added = self.spill.counts();
        let records = (stop.records - from.records, self.spill.subtrees.len());
        let entries = (
            stop.passed_down - from.passed_down,
            self.spill.passed_down.len(),
        );
        splice_items(scene, (from.items, stop.items), &mut self.spill);
        scene
            .subtrees
            .splice(from.records..stop.records, self.spill.subtrees.drain(..));
        scene.passed_down.splice(
            from.passed_down..stop.passed_down,
            self.spill.passed_down.drain(..),
        );
        let shift = |index: usize, (removed, added): (usize, usize)| index - removed + added;
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
        // The runs copied in place from the scene the walk started from no
        // longer lie where they did, nor each item where `reach` holds.
        scene.kept.clear();
        self.held = UpdatedReach::Lost;
        self.spill.clear();
    }

    // Hands back the reach list the rewrite took, whether it still holds
    // where each item of the scene reaches, and the areas it changed.
    pub(crate) fn results(
        &mut self,
    ) -> (Vec<Option<ClipBounds>>, UpdatedReach, &mut Vec<ClipBounds>) {
        (std::mem::take(&mut self.reach), self.held, &mut self.areas)
    }

    // Lets the spill's storage go.
    pub(crate) fn release_spill(&mut self) {
        self.spill = RasterScene::new();
    }

    // The storage of the spill, for a scene built anew.
    pub(crate) fn take_storage(&mut self) -> RasterScene {
        std::mem::take(&mut self.spill)
    }
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
    let placed = |item: RenderItem, base: ItemCounts| match item {
        RenderItem::Shape(index) => RenderItem::Shape(base.shapes + index),
        RenderItem::Image(index) => RenderItem::Image(base.images + index),
        RenderItem::Text(index) => RenderItem::Text(base.texts + index),
        RenderItem::Shadow(index) => RenderItem::Shadow(base.shadows + index),
    };
    scene.order.splice(
        to.order..old_stop.order,
        from.order.drain(..).map(|item| placed(item, to)),
    );
    // The items after the run lie after its items in each list.
    let shift = |index: usize, removed: usize, added: usize| index - removed + added;
    for item in &mut scene.order[to.order + added.order..] {
        *item = match *item {
            RenderItem::Shape(index) => {
                RenderItem::Shape(shift(index, removed.shapes, added.shapes))
            }
            RenderItem::Image(index) => {
                RenderItem::Image(shift(index, removed.images, added.images))
            }
            RenderItem::Text(index) => RenderItem::Text(shift(index, removed.texts, added.texts)),
            RenderItem::Shadow(index) => {
                RenderItem::Shadow(shift(index, removed.shadows, added.shadows))
            }
        };
    }
}
