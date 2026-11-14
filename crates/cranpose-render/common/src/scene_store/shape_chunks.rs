// The shapes of the draws that are plain data, a background's among them,
// recorded into chunks that the runs of the whole store share: a shape item
// names its chunk and its segments there, and a packet leases a chunk whole.
// A chunk takes new shapes only while no packet holds it, and a chunk that
// no item names any more takes new shapes again once no packet holds it.
// A background drawn anew each frame leaves its old shapes behind. The
// shapes an item took last go with the item at once. A chunk whose next
// shape would grow it, or that is full, first drops the shapes no item
// draws once they are a quarter of it and at least `MIN_DEAD_SHAPES`.

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
    // One bit a segment of the chunk that drops its dead shapes, set for
    // the segments a run names: room the next such chunk takes again.
    live: Vec<u64>,
}

#[derive(Clone)]
struct ShapeChunk {
    recorder: Arc<ShapeRecorder>,
    // How many items name the chunk, and how many shapes they draw.
    items: u32,
    shapes: u32,
    // The indexes of the runs that name the chunk, in no order: a chunk
    // drops its dead shapes without a look at the runs of other chunks.
    members: Vec<u32>,
}

impl ShapeChunks {
    // Records the shapes of one item with `record`, which draw what `print`
    // hashes, into a chunk that takes them, as the run at `at` of `runs`, a
    // place `Slab::lend` left empty, or else at a new place, which it
    // returns. Out of line: a background drawn again unchanged keeps its
    // shapes.
    #[inline(never)]
    fn record(
        &mut self,
        runs: &mut Slab<ShapeRun>,
        (at, print): (Option<usize>, u64),
        record: impl FnOnce(&mut ShapeRecorder),
    ) -> usize {
        let chunk = self.writable_chunk(runs);
        let entry = self.chunks[chunk.index()]
            .as_mut()
            .expect("a writable chunk is there");
        let recorder = Arc::get_mut(&mut entry.recorder).expect("no packet holds a writable chunk");
        let start = recorder.all_segments().end;
        record(recorder);
        recorder.end_segment();
        let segments = start..recorder.all_segments().end;
        let shapes = recorder.shape_count_in(&segments);
        entry.items += 1;
        entry.shapes += shapes;
        let run = ShapeRun {
            chunk,
            shapes,
            coverage: recorder.coverage_in(&segments).unwrap_or_default(),
            segments,
            print,
        };
        let index = match at {
            Some(index) => {
                runs.put(index, run);
                index
            }
            None => runs.insert(run),
        };
        entry
            .members
            .push(u32::try_from(index).expect("fewer than 2^32 shape items"));
        index
    }

    // A chunk with room that no packet holds: the open one, rid of the
    // shapes no item draws when it holds enough of them, an empty one
    // emptied of its old shapes, or a new one.
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
                keep_live_runs(recorder, runs, &mut chunk.members, &mut self.live);
                return open;
            }
            if !full {
                return open;
            }
        }
        let reused = self.empty.iter().position(|&id| {
            self.chunks[id.index()]
                .as_mut()
                .is_some_and(|chunk| Arc::get_mut(&mut chunk.recorder).is_some())
        });
        let id = match reused {
            Some(at) => {
                let id = self.empty.swap_remove(at);
                if let Some(recorder) = self.chunks[id.index()]
                    .as_mut()
                    .and_then(|chunk| Arc::get_mut(&mut chunk.recorder))
                {
                    recorder.clear();
                }
                id
            }
            None => self.add_chunk(),
        };
        self.open = Some(id);
        id
    }

    #[cold]
    fn add_chunk(&mut self) -> ChunkId {
        let chunk = Some(ShapeChunk {
            recorder: Arc::default(),
            items: 0,
            shapes: 0,
            members: Vec::new(),
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

    // Notes that the item of `run`, at `index` of the runs, went. The open
    // chunk no item names takes the next shapes from its start once no
    // packet holds it; another such chunk waits for reuse, and past the few
    // kept, it goes.
    fn release(&mut self, index: usize, run: &ShapeRun) {
        let chunk = run.chunk;
        let open = self.open == Some(chunk);
        let Some(entry) = self.chunks[chunk.index()].as_mut() else {
            return;
        };
        entry.items -= 1;
        entry.shapes -= run.shapes;
        if let Some(at) = entry
            .members
            .iter()
            .position(|&member| member as usize == index)
        {
            entry.members.swap_remove(at);
        }
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
        for id in self.empty.drain(..) {
            self.chunks[id.index()] = None;
        }
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
            let Some(recorder) = Arc::get_mut(&mut chunk.recorder) else {
                continue;
            };
            // Only a chunk that a quarter of dead shapes fills moves, as a
            // chunk that takes shapes does.
            let held = recorder.tables().shapes.len();
            if held - chunk.shapes as usize >= (held / 4).max(MIN_DEAD_SHAPES) {
                keep_live_runs(recorder, runs, &mut chunk.members, &mut self.live);
            }
            recorder.trim_growth();
            chunk.members.trim_growth_slack();
        }
        while self.chunks.last().is_some_and(Option::is_none) {
            self.chunks.pop();
        }
        self.chunks.trim_growth_slack();
        self.live = Vec::new();
    }

    // The heap the chunks hold: their lists and every chunk's recorder.
    fn heap_bytes(&self) -> usize {
        self.chunks.capacity() * size_of::<Option<ShapeChunk>>()
            + self.empty.capacity() * size_of::<ChunkId>()
            + self.live.capacity() * size_of::<u64>()
            + self
                .chunks
                .iter()
                .flatten()
                .map(|chunk| {
                    size_of::<ShapeRecorder>()
                        + chunk.recorder.tables().heap_bytes()
                        + chunk.members.capacity() * size_of::<u32>()
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

// Moves the shapes of `members`, the runs of a chunk, to the front of its
// `recorder`, in record order, and drops the shapes no run names. `live` is
// room for one bit a segment.
#[cold]
#[inline(never)]
fn keep_live_runs(
    recorder: &mut ShapeRecorder,
    runs: &mut Slab<ShapeRun>,
    members: &mut [u32],
    live: &mut Vec<u64>,
) {
    live.clear();
    live.resize(recorder.all_segments().end.div_ceil(64) as usize, 0);
    for &member in &*members {
        for segment in runs[member as usize].segments.clone() {
            live[segment as usize / 64] |= 1 << (segment % 64);
        }
    }
    recorder.keep_segments(|segment| live[segment as usize / 64] & (1 << (segment % 64)) != 0);
    // The runs take disjoint segments: in record order, each now starts
    // where the runs before it end.
    members.sort_unstable_by_key(|&member| runs[member as usize].segments.start);
    let mut start = 0;
    for &member in &*members {
        let run = &mut runs[member as usize];
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
        let index = self
            .shape_chunks
            .record(&mut self.shape_runs, (None, print), record);
        Item::new(ItemKind::Shape, index)
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
        self.shape_chunks.release(index, &old);
        self.shape_chunks
            .record(&mut self.shape_runs, (Some(index), print), record);
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
            self.shape_chunks.release(index, &run);
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
