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
// takes the shapes, or else a chunk the last sweep left with room. Without
// one, once the chunks hold more shapes no item draws than live ones and
// `SLACK_CHUNKS` full chunks, a sweep drops them from every chunk that holds
// at least as many of them as live ones and `MIN_DEAD_SHAPES`, before a new
// chunk takes the shapes: one pass over the runs finds the runs of every
// such chunk. The next sweep waits until as many shapes went again as lived
// then. So the chunks hold room for a few times the live shapes and a few
// chunks, however long a few items of a chunk live; a move frees room for
// as many new shapes as it moves or more, and a sweep runs once for as many
// shapes recorded as live ones. A store within that room moves nothing and
// pays nothing: its shapes go as its chunks empty, as a scene that redraws
// the same items each frame keeps it. A packet that holds a swept chunk
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
    // The chunks a sweep or a packet left with room, which take shapes
    // before a new chunk does.
    roomy: Vec<ChunkId>,
    // Whether an item went since a look for chunks of many dead shapes
    // found none or swept them.
    released: bool,
    // The dead shapes past which the next sweep may run: those the last
    // sweep left, and as many again as the shapes that lived then.
    next_sweep: usize,
    sweep: Sweep,
}

#[derive(Clone)]
struct ShapeChunk {
    recorder: Arc<ShapeRecorder>,
    // How many items name the chunk, and how many shapes they draw.
    items: u32,
    shapes: u32,
}

impl ShapeChunk {
    fn dead(&self) -> usize {
        self.recorder.tables().shapes.len() - self.shapes as usize
    }

    // Whether an item names the chunk and it takes more shapes.
    fn roomy(&self) -> bool {
        self.items > 0 && self.recorder.tables().shapes.len() < CHUNK_SHAPES
    }

    // Whether the chunk holds at least as many shapes no item draws as
    // live ones and `MIN_DEAD_SHAPES`: a move of its live shapes frees room
    // for as many new ones or more.
    fn sparse(&self) -> bool {
        self.items > 0 && self.dead() >= (self.shapes as usize).max(MIN_DEAD_SHAPES)
    }
}

// The runs of the store that name segments, by chunk, and where the runs of
// each chunk start among them: the scratch of a sweep, which the next one
// takes again.
#[derive(Clone, Default)]
struct Sweep {
    starts: Vec<u32>,
    runs: Vec<u32>,
}

impl Sweep {
    // Puts the runs that name segments in their chunks' places, `chunks`
    // places in all: a counting sort by chunk.
    fn bucket(&mut self, chunks: usize, runs: &Slab<ShapeRun>) {
        let Self {
            starts,
            runs: order,
        } = self;
        // Each chunk counts its runs two places past its own, so that once
        // a chunk's place one past its own walks its runs in, it marks
        // where the next chunk's runs start.
        starts.clear();
        starts.resize(chunks + 2, 0);
        for run in runs.values().filter(|run| !run.segments.is_empty()) {
            starts[run.chunk.index() + 2] += 1;
        }
        for index in 2..starts.len() {
            starts[index] += starts[index - 1];
        }
        order.clear();
        order.resize(starts[chunks + 1] as usize, 0);
        for (index, run) in runs.indexed().filter(|(_, run)| !run.segments.is_empty()) {
            let at = &mut starts[run.chunk.index() + 1];
            order[*at as usize] = u32::try_from(index).expect("fewer than 2^32 shape items");
            *at += 1;
        }
    }

    // The runs of the chunk at `index`, which `bucket` put in place.
    fn chunk_runs(&mut self, index: usize) -> &mut [u32] {
        &mut self.runs[self.starts[index] as usize..self.starts[index + 1] as usize]
    }

    fn heap_bytes(&self) -> usize {
        (self.starts.capacity() + self.runs.capacity()) * size_of::<u32>()
    }
}

impl ShapeChunks {
    // Records the shapes of one item with `record`, which draw what `print`
    // hashes, into a chunk that takes them, and returns where they went. An
    // item that records no shapes names no segment of its chunk, and its
    // run the empty range at the start, which every chunk holds.
    // Out of line: a background drawn again unchanged keeps its shapes.
    #[inline(never)]
    fn record(
        &mut self,
        runs: &mut Slab<ShapeRun>,
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
        entry.items += 1;
        entry.shapes += shapes;
        ShapeRun {
            chunk,
            shapes,
            coverage: recorder.coverage_in(&segments).unwrap_or_default(),
            segments,
            print,
        }
    }

    // A chunk with room that no packet holds: the open one, rid of the
    // shapes no item draws when it holds enough of them; else an empty one
    // emptied of its old shapes; else one a sweep left with room, after a
    // sweep when one is due; else a new one.
    fn writable_chunk(&mut self, runs: &mut Slab<ShapeRun>) -> ChunkId {
        let chunk_count = self.chunks.len();
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
                self.sweep.bucket(chunk_count, runs);
                compact(chunk, self.sweep.chunk_runs(open.index()), runs);
                return open;
            }
            if !full {
                return open;
            }
        }
        // An open chunk that a packet holds takes shapes again once the
        // packet lets go.
        if let Some(open) = self.open
            && self.chunks[open.index()]
                .as_ref()
                .is_some_and(ShapeChunk::roomy)
        {
            self.roomy.push(open);
        }
        let id = self
            .free_empty_chunk()
            .or_else(|| self.roomy_chunk())
            .or_else(|| {
                self.sweep(runs);
                self.roomy_chunk()
            })
            .unwrap_or_else(|| self.add_chunk());
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
        if let Some(recorder) = self.chunks[id.index()]
            .as_mut()
            .and_then(|chunk| Arc::get_mut(&mut chunk.recorder))
        {
            recorder.clear();
        }
        Some(id)
    }

    // A chunk with room that an item still names and no packet holds, among
    // those a sweep or a packet left with room. The list forgets the chunks
    // that filled or emptied, and keeps those a packet holds.
    fn roomy_chunk(&mut self) -> Option<ChunkId> {
        let chunks = &mut self.chunks;
        self.roomy.retain(|id| {
            chunks
                .get(id.index())
                .and_then(Option::as_ref)
                .is_some_and(ShapeChunk::roomy)
        });
        let at = self.roomy.iter().position(|id| {
            chunks[id.index()]
                .as_mut()
                .is_some_and(|chunk| Arc::get_mut(&mut chunk.recorder).is_some())
        })?;
        Some(self.roomy.swap_remove(at))
    }

    // Drops the shapes no item draws from every sparse chunk, once the
    // chunks hold more of them than the live shapes and `SLACK_CHUNKS` full
    // chunks, and more than `next_sweep`: one pass over the runs finds the
    // runs of every such chunk. The chunks it left with room take shapes
    // before a new chunk does. None is looked for while no item went since
    // a look found no such chunk.
    #[cold]
    #[inline(never)]
    fn sweep(&mut self, runs: &mut Slab<ShapeRun>) {
        if !self.released {
            return;
        }
        self.released = false;
        // The chunks no item names take shapes again as they are: their
        // shapes count as room, not as dead ones.
        let (mut dead, mut live, mut sparse) = (0, 0, false);
        for chunk in self.chunks.iter().flatten().filter(|chunk| chunk.items > 0) {
            dead += chunk.dead();
            live += chunk.shapes as usize;
            sparse |= chunk.sparse();
        }
        if !sparse || dead <= (live + SLACK_CHUNKS * CHUNK_SHAPES).max(self.next_sweep) {
            return;
        }
        self.sweep.bucket(self.chunks.len(), runs);
        for (index, slot) in self.chunks.iter_mut().enumerate() {
            let Some(chunk) = slot.as_mut().filter(|chunk| chunk.sparse()) else {
                continue;
            };
            dead -= chunk.dead();
            compact(chunk, self.sweep.chunk_runs(index), runs);
            self.roomy.push(ChunkId::at(index));
        }
        self.next_sweep = dead + live;
    }

    #[cold]
    fn add_chunk(&mut self) -> ChunkId {
        let chunk = Some(ShapeChunk {
            recorder: Arc::default(),
            items: 0,
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

    // Notes that the item of `run` went. The open chunk no item names takes
    // the next shapes from its start once no packet holds it; another such
    // chunk waits for reuse, and past the few kept, it goes.
    fn release(&mut self, run: &ShapeRun) {
        let chunk = run.chunk;
        let open = self.open == Some(chunk);
        let Some(entry) = self.chunks[chunk.index()].as_mut() else {
            return;
        };
        self.released = true;
        entry.items -= 1;
        entry.shapes -= run.shapes;
        if let Some(recorder) = Arc::get_mut(&mut entry.recorder) {
            if entry.items == 0 && open {
                recorder.clear();
                return;
            }
            if !run.segments.is_empty() && run.segments.end == recorder.all_segments().end {
                recorder.truncate_segments(run.segments.start);
            }
        }
        if entry.items > 0 {
            return;
        }
        if open {
            self.open = None;
        }
        if self.empty.len() < KEPT_EMPTY_CHUNKS {
            self.empty.push(chunk);
        } else {
            self.chunks[chunk.index()] = None;
        }
    }

    // Gives back at rest the chunks no item names and, in each chunk no
    // packet holds, the shapes no item draws and the room of its growth.
    // A packet keeps a chunk it holds until it lets go.
    #[cold]
    fn trim(&mut self, runs: &mut Slab<ShapeRun>) {
        self.empty.clear();
        self.roomy = Vec::new();
        self.sweep.bucket(self.chunks.len(), runs);
        for (index, slot) in self.chunks.iter_mut().enumerate() {
            let Some(chunk) = slot else {
                continue;
            };
            let id = ChunkId::at(index);
            if chunk.items == 0 {
                *slot = None;
                if self.open == Some(id) {
                    self.open = None;
                }
                continue;
            }
            if Arc::get_mut(&mut chunk.recorder).is_none() {
                continue;
            }
            // Only a chunk that a quarter of dead shapes fills moves, as a
            // chunk that takes shapes does.
            let held = chunk.recorder.tables().shapes.len();
            if chunk.dead() >= (held / 4).max(MIN_DEAD_SHAPES) {
                compact(chunk, self.sweep.chunk_runs(index), runs);
            }
            if let Some(recorder) = Arc::get_mut(&mut chunk.recorder) {
                recorder.trim_growth();
            }
        }
        while self.chunks.last().is_some_and(Option::is_none) {
            self.chunks.pop();
        }
        self.chunks.trim_growth_slack();
        self.sweep = Sweep::default();
        self.next_sweep = 0;
    }

    // The heap the chunks hold: their lists and every chunk's recorder.
    fn heap_bytes(&self) -> usize {
        self.chunks.capacity() * size_of::<Option<ShapeChunk>>()
            + (self.empty.capacity() + self.roomy.capacity()) * size_of::<ChunkId>()
            + self.sweep.heap_bytes()
            + self
                .chunks
                .iter()
                .flatten()
                .map(|chunk| size_of::<ShapeRecorder>() + chunk.recorder.tables().heap_bytes())
                .sum::<usize>()
    }

    fn recorder(&self, chunk: ChunkId) -> &Arc<ShapeRecorder> {
        &self.chunks[chunk.index()]
            .as_ref()
            .expect("a shape item names a live chunk")
            .recorder
    }
}

// Drops the shapes no item draws from `chunk`, whose runs `chunk_runs`
// names, its live shapes moved to its front in record order. A chunk that
// a packet holds stays as the packet reads it: a new recorder takes in its
// live shapes and its place, and the old one goes once the packet lets go.
#[cold]
#[inline(never)]
fn compact(chunk: &mut ShapeChunk, chunk_runs: &mut [u32], runs: &mut Slab<ShapeRun>) {
    chunk_runs.sort_unstable_by_key(|&index| runs[index as usize].segments.start);
    let kept = chunk_runs
        .iter()
        .map(|&index| runs[index as usize].segments.clone());
    if let Some(recorder) = Arc::get_mut(&mut chunk.recorder) {
        recorder.keep_segment_ranges(kept);
    } else {
        let mut fresh = ShapeRecorder::default();
        fresh.keep_segment_ranges_from(&chunk.recorder, kept);
        chunk.recorder = Arc::new(fresh);
    }
    let mut start = 0;
    for &index in chunk_runs.iter() {
        let run = &mut runs[index as usize];
        let end = start + (run.segments.end - run.segments.start);
        run.segments = start..end;
        start = end;
    }
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl SceneStore {
    /// Adds an item of the shapes `record` records into a chunk the store
    /// shares among its shape items. `print` is a hash of what they draw,
    /// the same for the same shapes.
    pub fn add_shapes(&mut self, print: u64, record: impl FnOnce(&mut ShapeRecorder)) -> Item {
        let run = self
            .shape_chunks
            .record(&mut self.shape_runs, print, record);
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
        self.shape_chunks.release(&old);
        let run = self
            .shape_chunks
            .record(&mut self.shape_runs, print, record);
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
