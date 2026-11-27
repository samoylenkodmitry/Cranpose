// The shapes of the draws that are plain data, a background's among them,
// in two lifetimes. A shape item records into the open chunk, a young chunk
// that the items recorded in the same frames share; a packet leases a chunk
// whole and reads it as it held it. A chunk takes new shapes only while no
// packet holds it, and a young chunk that no item names any more takes new
// shapes again once no packet holds it. A background drawn anew each frame
// leaves its old shapes behind; the shapes an item took last go with the
// item at once. The open chunk, once its next shape would grow it or it is
// full, first drops the shapes no item draws when they are a quarter of it
// and at least `MIN_DEAD_SHAPES`.
//
// A chunk no longer open whose items draw a quarter of its room or less
// waits `MOVE_AFTER_UPDATES` updates, as most shapes drawn anew die within
// a few frames, and then moves the items still there into an old chunk: one
// for each update, which takes all such items, sized to them and written
// once. A move records each item's shapes again from the primitives they
// came from. An item that outlives the others of its frame moves once, no
// chunk stays for it, and no frame pays for the items that stay. An old
// chunk that its items draw little of moves them the same way. A move reads
// a chunk and writes none that a packet holds.

use std::{collections::VecDeque, num::NonZeroU32, ops::Range, sync::Arc};

use cranpose_core::SceneUpdate;
use cranpose_ui_graphics::{DrawPrimitive, Rect, ShapeRecorder};

use super::{Item, ItemKind, SceneStore, slab::Slab};

// The most shapes a chunk takes: a chunk that one item still names keeps
// the room of this many until the item moves.
const CHUNK_SHAPES: usize = 256;
// The most young chunks no item names that the store keeps for reuse. A
// packet in flight holds the chunks it draws, so a frame that redraws a
// shape writes into an empty chunk that the packets of the frames before
// let go of; the chunks whose items moved empty in a burst.
const KEPT_EMPTY_CHUNKS: usize = 8;
// The most old chunks no item names that the store keeps for the next move.
const KEPT_SPARE_HOMES: usize = 8;
// The fewest shapes no item draws for which the open chunk moves the
// others: a chunk of a few shapes grows to room for this many before it
// does.
const MIN_DEAD_SHAPES: usize = 16;
// The updates a sparse chunk waits before its items move: of the shapes a
// gauntlet frame draws anew, 84 live one frame later, 18 two frames later
// and 3 from the third frame on, for long.
const MOVE_AFTER_UPDATES: u32 = 3;

// A chunk's place in the store's list of chunks, past 0, so a slab of
// runs needs no room for the absence of one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChunkId(NonZeroU32);

impl ChunkId {
    fn at(index: usize) -> Self {
        Self(
            u32::try_from(index + 1)
                .ok()
                .and_then(NonZeroU32::new)
                .expect("a store holds fewer than 2^32 shape chunks"),
        )
    }

    fn index(self) -> usize {
        self.0.get() as usize - 1
    }
}

/// The shapes of one shape item in its chunk.
#[derive(Clone, Debug)]
pub struct ShapeRun {
    chunk: ChunkId,
    // How many shapes the item draws, and the rect they cover when it
    // draws any: a collect reads them for every item it draws.
    shapes: u32,
    coverage: Rect,
    segments: Range<u32>,
    // A hash of what the shapes draw, which the chunk's other shapes leave
    // alone, from what recorded them.
    print: u64,
}

/// A shape item as a reader sees it: shapes recorded into a chunk that
/// other items share.
#[derive(Clone, Copy)]
pub struct ShapeRef<'a> {
    recorder: &'a Arc<ShapeRecorder>,
    run: &'a ShapeRun,
}

impl<'a> ShapeRef<'a> {
    /// The recorder of the chunk the shapes sit in, which a packet leases.
    pub fn recorder(self) -> &'a Arc<ShapeRecorder> {
        self.recorder
    }

    /// The segments of the chunk the shapes take.
    pub fn segments(self) -> Range<u32> {
        self.run.segments.clone()
    }

    /// How many shapes the item draws.
    #[inline]
    pub fn shape_count(self) -> u32 {
        self.run.shapes
    }

    /// The rect the item's pixels lie within, in its record's space; `None`
    /// for an item that draws nothing.
    #[inline]
    pub fn coverage(self) -> Option<Rect> {
        (self.run.shapes > 0).then_some(self.run.coverage)
    }

    /// A hash of what the item draws, which the other items of its chunk
    /// leave alone.
    #[inline]
    pub fn print(self) -> u64 {
        self.run.print
    }

    /// The primitives the item draws, in order.
    pub fn primitives(self) -> impl Iterator<Item = DrawPrimitive> + 'a {
        self.recorder.shape_primitives_in(&self.run.segments)
    }
}

#[derive(Default)]
pub(super) struct ShapeChunks {
    chunks: Slab<ShapeChunk>,
    // The chunk that took the last shapes.
    open: Option<ChunkId>,
    // The young chunks no item names, which take shapes again once no
    // packet holds them.
    empty: Vec<ChunkId>,
    // The old chunks no item names, which take the items of a move again
    // once no packet holds them.
    spare: Vec<ChunkId>,
    // The chunks that went sparse, each with the update it did in, oldest
    // first. A chunk that emptied since stays listed, and its entry passes.
    sparse: VecDeque<(ChunkId, SceneUpdate)>,
    // The store's update in progress.
    update: SceneUpdate,
    // The chunk of no shapes that the items which record none name, made
    // for the first such item: it takes no shapes and no item counts in it.
    nothing: Option<ChunkId>,
    // One bit a segment of the chunk that drops its dead shapes, set for
    // the segments a run names: room the next such chunk takes again.
    kept: Vec<u64>,
}

struct ShapeChunk {
    recorder: Arc<ShapeRecorder>,
    // How many items name the chunk, and how many shapes they draw.
    items: u32,
    shapes: u32,
    // The chunk waits to move its items once they draw fewer shapes than
    // this: a quarter of its room and one once it is no longer open, 0 while
    // it is open or waits.
    list_below: u32,
    // By segment, the item that recorded it: one entry a segment of the
    // recorder.
    owners: Vec<SegmentOwner>,
    // The update the chunk went sparse in while it waits to move its items.
    listed: Option<SceneUpdate>,
    age: ChunkAge,
}

enum ChunkAge {
    // A chunk that takes the shapes recorded while it is open.
    Young,
    // A chunk written once with the items of a move.
    Old,
}

// The item that recorded a segment: its index among the runs, with `LEADS`
// set on the item's first segment while the item names the chunk.
#[derive(Clone, Copy)]
struct SegmentOwner(u32);

const LEADS: u32 = 1 << 31;

impl SegmentOwner {
    fn run(self) -> usize {
        (self.0 & !LEADS) as usize
    }

    fn leads(self) -> bool {
        self.0 & LEADS != 0
    }
}

// The index among the runs of each item that `owners` marks as still in its
// chunk, in record order.
fn leading_runs(owners: &[SegmentOwner]) -> impl Iterator<Item = usize> + '_ {
    owners
        .iter()
        .filter(|owner| owner.leads())
        .map(|owner| owner.run())
}

// Whether no packet holds `recorder`: plain loads, where `Arc::get_mut`
// takes a lock on the counts.
fn is_unique(recorder: &Arc<ShapeRecorder>) -> bool {
    Arc::strong_count(recorder) == 1 && Arc::weak_count(recorder) == 0
}

impl ShapeChunk {
    fn new(recorder: ShapeRecorder, rows: usize, age: ChunkAge) -> Self {
        Self {
            recorder: Arc::new(recorder),
            items: 0,
            shapes: 0,
            list_below: 0,
            owners: Vec::with_capacity(rows),
            listed: None,
            age,
        }
    }

    // Waits to move its items once they draw a quarter of its room or less.
    fn list_when_sparse(&mut self) {
        self.list_below = u32::try_from(self.recorder.shape_capacity() / 4 + 1).unwrap_or(u32::MAX);
    }

    // Notes the item, the run at index `owner`, that took `segments`, the
    // segments at the end of the recorder, and draws `shapes` shapes.
    #[inline]
    fn take(&mut self, owner: usize, segments: &Range<u32>, shapes: u32) {
        let owner = u32::try_from(owner)
            .ok()
            .filter(|&owner| owner & LEADS == 0)
            .expect("a store holds fewer than 2^31 shape items");
        self.items += 1;
        self.shapes += shapes;
        self.owners.push(SegmentOwner(owner | LEADS));
        if segments.end - segments.start > 1 {
            self.take_later_segments(owner, segments.end);
        }
    }

    // Notes the item `owner` on its segments past the first, up to `end`.
    #[cold]
    fn take_later_segments(&mut self, owner: u32, end: u32) {
        self.owners.resize(end as usize, SegmentOwner(owner));
    }

    // Notes that the item of `run` went.
    fn drop_run(&mut self, run: &ShapeRun) {
        self.items -= 1;
        self.shapes -= run.shapes;
        self.owners[run.segments.start as usize].0 &= !LEADS;
    }

    // Forgets every item, for a chunk that takes shapes from its start.
    fn forget_items(&mut self) {
        self.items = 0;
        self.shapes = 0;
        self.list_below = 0;
        self.listed = None;
        self.owners.clear();
    }

    // Empties the chunk no packet holds, to take shapes from its start.
    fn reset(&mut self) {
        if let Some(recorder) = Arc::get_mut(&mut self.recorder) {
            recorder.clear();
        }
        self.forget_items();
    }
}

impl ShapeChunks {
    // Records the shapes of one item, the run at index `owner`, with
    // `record`, which draw what `print` hashes, into a chunk that takes
    // them, and returns where they went. Out of line and one body for every
    // caller: a background drawn again unchanged keeps its shapes.
    #[inline(never)]
    fn record(
        &mut self,
        runs: &mut Slab<ShapeRun>,
        (owner, print): (usize, u64),
        record: &mut dyn FnMut(&mut ShapeRecorder),
    ) -> ShapeRun {
        let chunk = self.writable_chunk(runs);
        let entry = &mut self.chunks[chunk.index()];
        let recorder = Arc::get_mut(&mut entry.recorder).expect("no packet holds a writable chunk");
        let start = recorder.all_segments().end;
        let first_shape = recorder.tables().shapes.len();
        record(recorder);
        recorder.end_segment();
        let segments = start..recorder.all_segments().end;
        if segments.is_empty() {
            return ShapeRun {
                chunk: self.nothing(),
                shapes: 0,
                coverage: Rect::default(),
                segments: 0..0,
                print,
            };
        }
        // The item's segments hold the shapes it appended, as each item
        // starts a segment of its own.
        let shapes = u32::try_from(recorder.tables().shapes.len() - first_shape)
            .expect("an item records fewer than 2^32 shapes");
        let coverage = recorder.coverage_from(first_shape).unwrap_or_default();
        entry.take(owner, &segments, shapes);
        ShapeRun {
            chunk,
            shapes,
            coverage,
            segments,
            print,
        }
    }

    // A chunk with room that no packet holds: the open one, rid of the
    // shapes no item draws when it holds enough of them, an empty one
    // emptied of its old shapes, or a new one.
    fn writable_chunk(&mut self, runs: &mut Slab<ShapeRun>) -> ChunkId {
        if let Some(open) = self.open {
            let chunk = &mut self.chunks[open.index()];
            if is_unique(&chunk.recorder) {
                let held = chunk.recorder.tables().shapes.len();
                let full = held >= CHUNK_SHAPES;
                let dead = held - chunk.shapes as usize;
                if (full || held >= chunk.recorder.shape_capacity())
                    && dead >= (held / 4).max(MIN_DEAD_SHAPES)
                {
                    if let Some(recorder) = Arc::get_mut(&mut chunk.recorder) {
                        keep_live_runs(recorder, &mut chunk.owners, runs, &mut self.kept);
                    }
                    return open;
                }
                if !full {
                    return open;
                }
            }
            self.close(open);
        }
        let reused = self
            .empty
            .iter()
            .position(|&id| is_unique(&self.chunks[id.index()].recorder));
        let id = match reused {
            Some(at) => {
                let id = self.empty.swap_remove(at);
                self.chunks[id.index()].reset();
                id
            }
            None => self.insert_chunk(0, CHUNK_SHAPES, ChunkAge::Young),
        };
        self.open = Some(id);
        id
    }

    #[cold]
    fn nothing(&mut self) -> ChunkId {
        match self.nothing {
            Some(id) => id,
            None => {
                let id = self.insert_chunk(0, 0, ChunkAge::Old);
                self.nothing = Some(id);
                id
            }
        }
    }

    // A new chunk with room for `shapes` shapes in as many segments and for
    // `rows` owner entries.
    #[cold]
    fn insert_chunk(&mut self, shapes: usize, rows: usize, age: ChunkAge) -> ChunkId {
        let recorder = ShapeRecorder::with_capacity(shapes, shapes);
        ChunkId::at(self.chunks.insert(ShapeChunk::new(recorder, rows, age)))
    }

    // Leaves the open chunk `id`: kept for reuse when no item names it,
    // else listed when its items draw little of it.
    #[cold]
    fn close(&mut self, id: ChunkId) {
        self.open = None;
        let chunk = &mut self.chunks[id.index()];
        if chunk.items == 0 {
            self.keep_empty(id);
            return;
        }
        chunk.list_when_sparse();
        if chunk.shapes < chunk.list_below {
            self.list(id);
        }
    }

    // Lists the chunk `id` to move its items `MOVE_AFTER_UPDATES` updates
    // on.
    #[cold]
    fn list(&mut self, id: ChunkId) {
        let chunk = &mut self.chunks[id.index()];
        chunk.list_below = 0;
        chunk.listed = Some(self.update);
        self.sparse.push_back((id, self.update));
    }

    // Notes that the item of `run` went. The open chunk no item names takes
    // the next shapes from its start once no packet holds it; another such
    // chunk waits for reuse, and past the few kept, it goes. A chunk no
    // longer open that its items draw little of waits to move them.
    fn release(&mut self, run: &ShapeRun) {
        // An item of no shapes takes no room.
        if run.segments.is_empty() {
            return;
        }
        let id = run.chunk;
        let open = self.open == Some(id);
        let entry = &mut self.chunks[id.index()];
        entry.drop_run(run);
        let emptied = entry.items == 0 && open;
        if (emptied || run.segments.end == entry.recorder.all_segments().end)
            && let Some(recorder) = Arc::get_mut(&mut entry.recorder)
        {
            if emptied {
                recorder.clear();
                entry.forget_items();
                return;
            }
            recorder.truncate_segments(run.segments.start);
            entry.owners.truncate(run.segments.start as usize);
        }
        if entry.items > 0 {
            if entry.shapes < entry.list_below {
                self.list(id);
            }
            return;
        }
        entry.listed = None;
        if open {
            self.open = None;
        }
        self.keep_empty(id);
    }

    // Keeps the chunk `id` that no item names for reuse, or past the few
    // kept, lets it go.
    #[cold]
    fn keep_empty(&mut self, id: ChunkId) {
        let (kept, most) = match self.chunks[id.index()].age {
            ChunkAge::Young => (&mut self.empty, KEPT_EMPTY_CHUNKS),
            ChunkAge::Old => (&mut self.spare, KEPT_SPARE_HOMES),
        };
        if kept.len() < most {
            kept.push(id);
        } else {
            self.chunks.remove(id.index());
        }
    }

    // Whether the first listed chunk went sparse at least `after` updates
    // ago.
    fn any_due(&self, after: u32) -> bool {
        self.sparse
            .front()
            .is_some_and(|&(_, listed)| self.update.since(listed) >= after)
    }

    // How many of the listed chunks went sparse at least `after` updates
    // ago, the first of the list.
    fn due(&self, after: u32) -> usize {
        self.sparse
            .iter()
            .take_while(|&&(_, listed)| self.update.since(listed) >= after)
            .count()
    }

    // The chunk `id` while it waits in the entry `listed` of the list.
    fn listed_chunk(&self, id: ChunkId, listed: SceneUpdate) -> Option<&ShapeChunk> {
        self.chunks
            .slot(id.index())
            .filter(|chunk| chunk.listed == Some(listed))
    }

    // Moves the items of the chunks that went sparse at least `after`
    // updates ago into one old chunk, and empties those chunks. Out of
    // line: most updates move nothing.
    #[inline(never)]
    fn move_sparse_items(&mut self, runs: &mut Slab<ShapeRun>, after: u32) {
        let due = self.due(after);
        let (items, shapes) = self
            .sparse
            .range(..due)
            .filter_map(|&(id, listed)| self.listed_chunk(id, listed))
            .fold((0, 0), |(items, shapes), chunk| {
                (items + chunk.items, shapes + chunk.shapes as usize)
            });
        let mut home = (items > 0).then(|| {
            let home = self.home(shapes);
            (
                home,
                self.chunks.lend(home.index()).expect("a home is there"),
            )
        });
        // A chunk listed twice in one update moves once: its move forgets
        // the update it was listed in.
        for _ in 0..due {
            let Some((id, listed)) = self.sparse.pop_front() else {
                break;
            };
            let Some(chunk) = self.listed_chunk(id, listed) else {
                continue;
            };
            if let Some((home, old)) = home.as_mut() {
                move_runs(chunk, runs, *home, old);
            }
            self.chunks[id.index()].forget_items();
            self.keep_empty(id);
        }
        if let Some((home, mut old)) = home {
            old.list_when_sparse();
            self.chunks.put(home.index(), old);
        }
    }

    // An empty old chunk with room for `shapes` shapes and as many
    // segments, as each segment holds a shape: a spare one no packet holds
    // with that room, up to twice that rounded up to a power of two, which
    // the shapes still fill past a quarter, or a new one with room for that
    // power of two.
    fn home(&mut self, shapes: usize) -> ChunkId {
        let room = shapes.next_power_of_two();
        let spare = self.spare.iter().position(|&id| {
            let recorder = &self.chunks[id.index()].recorder;
            is_unique(recorder)
                && (shapes..=2 * room).contains(&recorder.shape_capacity())
                && recorder.tables().segments.capacity() >= shapes
        });
        match spare {
            Some(at) => {
                let id = self.spare.swap_remove(at);
                self.chunks[id.index()].reset();
                id
            }
            None => self.insert_chunk(room, room, ChunkAge::Old),
        }
    }

    // Gives back at rest the chunks no item names and, in each chunk no
    // packet holds, the shapes no item draws and the room of its growth.
    // The items of the sparse chunks move first. A packet keeps a chunk it
    // holds until it lets go.
    #[cold]
    fn trim(&mut self, runs: &mut Slab<ShapeRun>) {
        self.move_sparse_items(runs, 0);
        for id in self.empty.drain(..).chain(self.spare.drain(..)) {
            self.chunks.remove(id.index());
        }
        for index in 0..self.chunks.slot_count() {
            let Some(chunk) = self.chunks.slot_mut(index) else {
                continue;
            };
            let id = Some(ChunkId::at(index));
            let open = self.open == id;
            if chunk.items == 0 && self.nothing != id {
                self.chunks.remove(index);
                if open {
                    self.open = None;
                }
                continue;
            }
            let Some(recorder) = Arc::get_mut(&mut chunk.recorder) else {
                continue;
            };
            // Each move reads every run of the chunk: only a chunk that a
            // quarter of dead shapes fills moves, as a chunk that takes
            // shapes does.
            let held = recorder.tables().shapes.len();
            if held - chunk.shapes as usize >= (held / 4).max(MIN_DEAD_SHAPES) {
                keep_live_runs(recorder, &mut chunk.owners, runs, &mut self.kept);
            }
            recorder.trim_growth();
            if !open {
                chunk.list_when_sparse();
            }
        }
        self.chunks.cut_free_tail();
        self.chunks.trim_growth_slack();
        self.kept = Vec::new();
    }

    // The heap the chunks hold: their lists and every chunk's recorder.
    fn heap_bytes(&self) -> usize {
        self.chunks.heap_bytes()
            + (self.empty.capacity() + self.spare.capacity()) * size_of::<ChunkId>()
            + self.sparse.capacity() * size_of::<(ChunkId, SceneUpdate)>()
            + self.kept.capacity() * size_of::<u64>()
            + self
                .chunks
                .values()
                .map(|chunk| {
                    size_of::<ShapeRecorder>()
                        + chunk.recorder.tables().heap_bytes()
                        + chunk.owners.capacity() * size_of::<SegmentOwner>()
                })
                .sum::<usize>()
    }

    fn recorder(&self, chunk: ChunkId) -> &Arc<ShapeRecorder> {
        &self.chunks[chunk.index()].recorder
    }
}

// Records the shapes of every item of `chunk` again into `old`, the old
// chunk at `home`, after the shapes it holds, from the primitives they were
// recorded from.
fn move_runs(chunk: &ShapeChunk, runs: &mut Slab<ShapeRun>, home: ChunkId, old: &mut ShapeChunk) {
    for owner in leading_runs(&chunk.owners).take(chunk.items as usize) {
        let run = &mut runs[owner];
        let recorder = Arc::get_mut(&mut old.recorder).expect("no packet holds a new home");
        let start = recorder.all_segments().end;
        recorder.push_shapes_of(&chunk.recorder, &run.segments);
        recorder.end_segment();
        run.segments = start..recorder.all_segments().end;
        run.chunk = home;
        old.take(owner, &run.segments, run.shapes);
    }
}

// Moves the shapes of the items that `owners` marks to the front of
// `recorder`, in record order, and drops the shapes no item draws. `kept`
// is room for one bit a segment.
#[cold]
#[inline(never)]
fn keep_live_runs(
    recorder: &mut ShapeRecorder,
    owners: &mut Vec<SegmentOwner>,
    runs: &mut Slab<ShapeRun>,
    kept: &mut Vec<u64>,
) {
    kept.clear();
    kept.resize(recorder.all_segments().end.div_ceil(64) as usize, 0);
    for run in leading_runs(owners) {
        for segment in runs[run].segments.clone() {
            kept[segment as usize / 64] |= 1 << (segment % 64);
        }
    }
    recorder.keep_segments(|segment| kept[segment as usize / 64] & (1 << (segment % 64)) != 0);
    // Each item now starts past the kept segments before it. A start only
    // moves down, so the owners are written over where they were read.
    let mut next = 0;
    for at in 0..owners.len() {
        let owner = owners[at];
        if !owner.leads() {
            continue;
        }
        let run = &mut runs[owner.run()];
        let len = run.segments.end - run.segments.start;
        run.segments = next..next + len;
        owners[next as usize] = owner;
        owners[(next + 1) as usize..(next + len) as usize].fill(SegmentOwner(owner.0 & !LEADS));
        next += len;
    }
    owners.truncate(next as usize);
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl SceneStore {
    /// Adds an item of the shapes `record` records into a chunk the store
    /// shares among its shape items. `print` is a hash of what they draw,
    /// the same for the same shapes.
    pub fn add_shapes(&mut self, print: u64, mut record: impl FnMut(&mut ShapeRecorder)) -> Item {
        let index = self.shape_runs.reserve();
        let run = self
            .shape_chunks
            .record(&mut self.shape_runs, (index, print), &mut record);
        self.shape_runs.put(index, run);
        Item::new(ItemKind::Shape, index)
    }

    // Records the shapes of the shape item `item` anew through `record`,
    // as `print` names them: the item keeps its handle, and its old shapes
    // go at once.
    pub(crate) fn redraw_shapes(
        &mut self,
        item: Item,
        print: u64,
        mut record: impl FnMut(&mut ShapeRecorder),
    ) {
        let index = item.index();
        let Some(old) = self.shape_runs.lend(index) else {
            return;
        };
        self.shape_chunks.release(&old);
        let run = self
            .shape_chunks
            .record(&mut self.shape_runs, (index, print), &mut record);
        self.shape_runs.put(index, run);
    }

    // A hash of what the shape item `item` draws: two items of one print
    // draw alike.
    #[inline]
    pub(crate) fn shapes_print(&self, item: Item) -> u64 {
        self.shape_runs[item.index()].print
    }

    /// The shapes of a shape item.
    #[inline]
    pub fn shapes(&self, item: Item) -> ShapeRef<'_> {
        let run = &self.shape_runs[item.index()];
        ShapeRef {
            recorder: self.shape_chunks.recorder(run.chunk),
            run,
        }
    }

    // Removes the shape item at `index` of the shape runs and lets go of
    // its shapes.
    pub(super) fn remove_shapes(&mut self, index: usize) {
        if let Some(run) = self.shape_runs.take(index) {
            self.shape_chunks.release(&run);
        }
    }

    // Notes the update the store began, and moves the items of the chunks
    // that went sparse `MOVE_AFTER_UPDATES` updates before or earlier.
    #[inline]
    pub(super) fn begin_shapes_update(&mut self, update: SceneUpdate) {
        let chunks = &mut self.shape_chunks;
        chunks.update = update;
        if chunks.any_due(MOVE_AFTER_UPDATES) {
            chunks.move_sparse_items(&mut self.shape_runs, MOVE_AFTER_UPDATES);
        }
    }

    // The heap the shape items and their chunks hold.
    pub(super) fn shapes_heap_bytes(&self) -> usize {
        self.shape_runs.heap_bytes() + self.shape_chunks.heap_bytes()
    }

    // Gives back at rest the room of the shapes no item draws.
    pub(super) fn trim_shapes(&mut self) {
        self.shape_chunks.trim(&mut self.shape_runs);
    }

    pub(super) fn shape_item_count(&self) -> usize {
        self.shape_runs.values().count()
    }

    pub(super) fn clear_shapes(&mut self) {
        self.shape_runs.clear();
        self.shape_chunks = ShapeChunks::default();
    }
}
