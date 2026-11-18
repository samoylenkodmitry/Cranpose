// The shapes of the draws that are plain data, a background's among them,
// recorded into chunks that the runs of the whole store share: a shape item
// names its chunk and its segments there, and a packet leases a chunk whole.
// A chunk takes new shapes only while no packet holds it, and a chunk that
// no item names any more takes new shapes again once no packet holds it.
// A background drawn anew each frame leaves its old shapes behind. The
// shapes an item took last go with the item at once. A chunk whose next
// shape would grow it, or that is full, first drops the shapes no item
// draws once they are a quarter of it and at least `MIN_DEAD_SHAPES`.
// When it cannot take the next shape, an empty chunk that no packet holds
// takes the shapes. Without one, once the chunks hold room for more than
// twice the live shapes and `SLACK_CHUNKS` full chunks, the chunk with the
// most shapes no item draws, at least as many as its live ones and
// `MIN_DEAD_SHAPES`, drops them and takes the shapes, before a new chunk
// does. So the chunks hold room for at most about twice the live shapes and
// a few chunks, however long a few items of a chunk live. A move frees room
// for as many new shapes as it moves or more. A store within that room
// moves nothing: its shapes go as its chunks empty, as a scene that redraws
// the same items each frame keeps it. A packet that holds such a chunk
// reads it as it was: a new recorder takes in the live shapes and the
// chunk's place.

use std::{num::NonZeroU32, ops::Range, sync::Arc};

use cranpose_core::collections::growth_slack::GrowthSlack;
use cranpose_ui_graphics::{DrawPrimitive, Rect, ShapeRecorder};

use super::{Item, ItemKind, SceneStore, slab::Slab};

// The most shapes a chunk takes: a chunk that one item still names keeps
// the room of this many.
const CHUNK_SHAPES: usize = 256;
// The most chunks no item names that the store keeps for reuse.
// A packet in flight holds the chunks it draws, so a frame that redraws a
// shape writes into an empty chunk that the packets of the frames before
// let go of.
const KEPT_EMPTY_CHUNKS: usize = 4;
// The fewest shapes no item draws for which a chunk moves the others: a
// chunk of a few shapes grows to room for this many before it does.
const MIN_DEAD_SHAPES: usize = 16;
// The full chunks of room past twice the live shapes that the chunks hold
// before a chunk of many dead shapes drops them in place of a new chunk.
const SLACK_CHUNKS: usize = 4;

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
    // The run's place in its chunk's list of runs.
    slot: u32,
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

#[derive(Clone, Default)]
pub(super) struct ShapeChunks {
    // By chunk id; `None` for an id whose chunk went.
    chunks: Vec<Option<ShapeChunk>>,
    // The chunk that took the last shapes.
    open: Option<ChunkId>,
    // The chunks no item names, which take shapes again once no packet
    // holds them.
    empty: Vec<ChunkId>,
    // Whether an item went since a look for a chunk of many dead shapes
    // found none.
    released: bool,
    // How many chunks the store holds, and how many shapes its items draw.
    chunk_count: usize,
    live_shapes: usize,
}

#[derive(Clone)]
struct ShapeChunk {
    recorder: Arc<ShapeRecorder>,
    // The index among the runs of each item that names the chunk, in no
    // order: a run knows its place here as its `slot`.
    runs: Vec<u32>,
    // How many shapes those items draw.
    shapes: u32,
}

// Sorts the runs of a chunk, `chunk_runs`, by their first segment, so a
// move of their segments to the front keeps them in order, and tells each
// run its new place.
fn sort_runs(chunk_runs: &mut [u32], runs: &mut Slab<ShapeRun>) {
    chunk_runs.sort_unstable_by_key(|&index| runs[index as usize].segments.start);
    for (slot, &index) in (0..).zip(chunk_runs.iter()) {
        runs[index as usize].slot = slot;
    }
}

// The segments of the sorted runs `chunk_runs`, in order.
fn run_segments<'a>(
    chunk_runs: &'a [u32],
    runs: &'a Slab<ShapeRun>,
) -> impl Iterator<Item = Range<u32>> + Clone + 'a {
    chunk_runs
        .iter()
        .map(|&index| runs[index as usize].segments.clone())
}

// Starts each of the sorted runs `chunk_runs` past the segments of the runs
// before it, once their chunk kept their segments alone.
fn renumber(chunk_runs: &[u32], runs: &mut Slab<ShapeRun>) {
    let mut start = 0;
    for &index in chunk_runs {
        let run = &mut runs[index as usize];
        if !run.segments.is_empty() {
            let end = start + (run.segments.end - run.segments.start);
            run.segments = start..end;
            start = end;
        }
    }
}

impl ShapeChunks {
    // Records the shapes of one item with `record`, which draw what `print`
    // hashes, into a chunk that takes them, and returns where they went: the
    // run that takes the index `owner` among the runs. An item that records
    // no shapes names no segment of its chunk, and its run the empty range
    // at the start, which every chunk holds.
    // Out of line: a background drawn again unchanged keeps its shapes.
    #[inline(never)]
    fn record(
        &mut self,
        runs: &mut Slab<ShapeRun>,
        owner: usize,
        print: u64,
        record: impl FnOnce(&mut ShapeRecorder),
    ) -> ShapeRun {
        let chunk = self.writable_chunk(runs);
        let entry = self.chunks[chunk.index()]
            .as_mut()
            .expect("a writable chunk is there");
        let recorder = Arc::get_mut(&mut entry.recorder).expect("no packet holds a writable chunk");
        let start = recorder.all_segments().end;
        record(recorder);
        recorder.end_segment();
        let end = recorder.all_segments().end;
        let segments = if end > start { start..end } else { 0..0 };
        let shapes = recorder.shape_count_in(&segments);
        let slot = u32::try_from(entry.runs.len()).expect("fewer than 2^32 shape items");
        entry
            .runs
            .push(u32::try_from(owner).expect("fewer than 2^32 shape items"));
        entry.shapes += shapes;
        self.live_shapes += shapes as usize;
        ShapeRun {
            chunk,
            slot,
            shapes,
            coverage: recorder.coverage_in(&segments).unwrap_or_default(),
            segments,
            print,
        }
    }

    // A chunk with room that no packet holds: the open one, rid of the
    // shapes no item draws when it holds enough of them; else an empty one
    // emptied of its old shapes; else the chunk of the most such shapes, rid
    // of them; else a new one.
    fn writable_chunk(&mut self, runs: &mut Slab<ShapeRun>) -> ChunkId {
        if let Some(open) = self.open
            && let Some(chunk) = self.chunks[open.index()].as_mut()
            && let Some(recorder) = Arc::get_mut(&mut chunk.recorder)
        {
            let held = recorder.tables().shapes.len();
            let full = held >= CHUNK_SHAPES;
            let dead = held - chunk.shapes as usize;
            if (full || held >= recorder.shape_capacity())
                && dead >= (held / 4).max(MIN_DEAD_SHAPES)
            {
                keep_live_runs(recorder, &mut chunk.runs, runs);
                return open;
            }
            if !full {
                return open;
            }
        }
        let id = match self.free_empty_chunk() {
            Some(empty) => empty,
            None => match self.sparse_chunk() {
                Some(sparse) => {
                    self.compact(sparse, runs);
                    sparse
                }
                None => self.add_chunk(),
            },
        };
        self.open = Some(id);
        id
    }

    // An empty chunk that no packet holds, emptied of its old shapes.
    fn free_empty_chunk(&mut self) -> Option<ChunkId> {
        let at = self.empty.iter().position(|&id| {
            self.chunks[id.index()]
                .as_mut()
                .is_some_and(|chunk| Arc::get_mut(&mut chunk.recorder).is_some())
        })?;
        let id = self.empty.swap_remove(at);
        if let Some(chunk) = self.chunks[id.index()].as_mut()
            && let Some(recorder) = Arc::get_mut(&mut chunk.recorder)
        {
            recorder.clear();
        }
        Some(id)
    }

    // The chunk of the most shapes no item draws, among those that hold at
    // least as many of them as live ones and `MIN_DEAD_SHAPES`: a move of
    // its live shapes frees room for as many new ones or more. None while
    // the chunks hold room for twice the live shapes and `SLACK_CHUNKS` full
    // chunks or less. None is looked for while no item went since a look
    // found no such chunk.
    fn sparse_chunk(&mut self) -> Option<ChunkId> {
        if !self.released
            || self.chunk_count * CHUNK_SHAPES <= 2 * self.live_shapes + SLACK_CHUNKS * CHUNK_SHAPES
        {
            return None;
        }
        let sparse = self
            .chunks
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| {
                let chunk = slot.as_ref().filter(|chunk| !chunk.runs.is_empty())?;
                let live = chunk.shapes as usize;
                let dead = chunk.recorder.tables().shapes.len() - live;
                (dead >= live.max(MIN_DEAD_SHAPES)).then_some((dead, index))
            })
            .max()
            .map(|(_, index)| ChunkId::at(index));
        self.released = sparse.is_some();
        sparse
    }

    // Drops the shapes no item draws from the chunk `id`, its live shapes
    // moved to its front. A chunk that a packet holds stays as the packet
    // reads it: a new recorder takes in its live shapes and its place, and
    // the old one goes once the packet lets go. No empty chunk could serve
    // in its place, as the store looks for this chunk only without one.
    #[cold]
    #[inline(never)]
    fn compact(&mut self, id: ChunkId, runs: &mut Slab<ShapeRun>) {
        let chunk = self.chunks[id.index()]
            .as_mut()
            .expect("a chunk an item names is there");
        if let Some(recorder) = Arc::get_mut(&mut chunk.recorder) {
            keep_live_runs(recorder, &mut chunk.runs, runs);
            return;
        }
        sort_runs(&mut chunk.runs, runs);
        let mut kept = ShapeRecorder::default();
        kept.keep_segment_ranges_from(&chunk.recorder, run_segments(&chunk.runs, runs));
        chunk.recorder = Arc::new(kept);
        renumber(&chunk.runs, runs);
    }

    #[cold]
    fn add_chunk(&mut self) -> ChunkId {
        self.chunk_count += 1;
        let chunk = Some(ShapeChunk {
            recorder: Arc::default(),
            runs: Vec::new(),
            shapes: 0,
        });
        let id = match self.chunks.iter().position(Option::is_none) {
            Some(id) => {
                self.chunks[id] = chunk;
                id
            }
            None => {
                self.chunks.push(chunk);
                self.chunks.len() - 1
            }
        };
        ChunkId::at(id)
    }

    // Notes that the item of `run`, which `runs` no longer holds, went. The
    // open chunk no item names takes the next shapes from its start once no
    // packet holds it; another such chunk waits for reuse, and past the few
    // kept, it goes.
    fn release(&mut self, run: &ShapeRun, runs: &mut Slab<ShapeRun>) {
        let chunk = run.chunk;
        let open = self.open == Some(chunk);
        let Some(entry) = self.chunks[chunk.index()].as_mut() else {
            return;
        };
        self.released = true;
        entry.runs.swap_remove(run.slot as usize);
        if let Some(&moved) = entry.runs.get(run.slot as usize) {
            runs[moved as usize].slot = run.slot;
        }
        entry.shapes -= run.shapes;
        self.live_shapes -= run.shapes as usize;
        if let Some(recorder) = Arc::get_mut(&mut entry.recorder) {
            if entry.runs.is_empty() && open {
                recorder.clear();
                return;
            }
            if !run.segments.is_empty() && run.segments.end == recorder.all_segments().end {
                recorder.truncate_segments(run.segments.start);
            }
        }
        if !entry.runs.is_empty() {
            return;
        }
        if open {
            self.open = None;
        }
        if self.empty.len() < KEPT_EMPTY_CHUNKS {
            self.empty.push(chunk);
        } else {
            self.chunks[chunk.index()] = None;
            self.chunk_count -= 1;
        }
    }

    // Gives back at rest the chunks no item names and, in each chunk no
    // packet holds, the shapes no item draws and the room of its growth.
    // A packet keeps a chunk it holds until it lets go.
    #[cold]
    fn trim(&mut self, runs: &mut Slab<ShapeRun>) {
        self.empty.clear();
        for (index, slot) in self.chunks.iter_mut().enumerate() {
            let Some(chunk) = slot else {
                continue;
            };
            let id = ChunkId::at(index);
            if chunk.runs.is_empty() {
                *slot = None;
                self.chunk_count -= 1;
                if self.open == Some(id) {
                    self.open = None;
                }
                continue;
            }
            let Some(recorder) = Arc::get_mut(&mut chunk.recorder) else {
                continue;
            };
            // Only a chunk that a quarter of dead shapes fills moves, as a
            // chunk that takes shapes does.
            let held = recorder.tables().shapes.len();
            if held - chunk.shapes as usize >= (held / 4).max(MIN_DEAD_SHAPES) {
                keep_live_runs(recorder, &mut chunk.runs, runs);
            }
            recorder.trim_growth();
            chunk.runs.trim_growth_slack();
        }
        while self.chunks.last().is_some_and(Option::is_none) {
            self.chunks.pop();
        }
        self.chunks.trim_growth_slack();
    }

    // The heap the chunks hold: their lists and every chunk's recorder.
    fn heap_bytes(&self) -> usize {
        self.chunks.capacity() * size_of::<Option<ShapeChunk>>()
            + self.empty.capacity() * size_of::<ChunkId>()
            + self
                .chunks
                .iter()
                .flatten()
                .map(|chunk| {
                    size_of::<ShapeRecorder>()
                        + chunk.recorder.tables().heap_bytes()
                        + chunk.runs.capacity() * size_of::<u32>()
                })
                .sum::<usize>()
    }

    fn recorder(&self, chunk: ChunkId) -> &Arc<ShapeRecorder> {
        &self.chunks[chunk.index()]
            .as_ref()
            .expect("a shape item names a live chunk")
            .recorder
    }
}

// Moves the shapes of `chunk_runs`, the runs of the chunk of `recorder`, to
// the front of it, in record order, and drops the shapes no run names.
#[cold]
#[inline(never)]
fn keep_live_runs(recorder: &mut ShapeRecorder, chunk_runs: &mut [u32], runs: &mut Slab<ShapeRun>) {
    sort_runs(chunk_runs, runs);
    recorder.keep_segment_ranges(run_segments(chunk_runs, runs));
    renumber(chunk_runs, runs);
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl SceneStore {
    /// Adds an item of the shapes `record` records into a chunk the store
    /// shares among its shape items. `print` is a hash of what they draw,
    /// the same for the same shapes.
    pub fn add_shapes(&mut self, print: u64, record: impl FnOnce(&mut ShapeRecorder)) -> Item {
        let owner = self.shape_runs.vacant();
        let run = self
            .shape_chunks
            .record(&mut self.shape_runs, owner, print, record);
        Item::new(ItemKind::Shape, self.shape_runs.insert(run))
    }

    // Records the shapes of the shape item `item` anew through `record`,
    // as `print` names them: the item keeps its handle, and its old shapes
    // go at once.
    pub(crate) fn redraw_shapes(
        &mut self,
        item: Item,
        print: u64,
        record: impl FnOnce(&mut ShapeRecorder),
    ) {
        let index = item.index();
        let Some(old) = self.shape_runs.lend(index) else {
            return;
        };
        self.shape_chunks.release(&old, &mut self.shape_runs);
        let run = self
            .shape_chunks
            .record(&mut self.shape_runs, index, print, record);
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
            self.shape_chunks.release(&run, &mut self.shape_runs);
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
