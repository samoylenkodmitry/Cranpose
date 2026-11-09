// The shapes of the draws that are plain data, a background's among them,
// recorded into chunks that the runs of the whole store share: a shape item
// names its chunk and its segments there, and a packet leases a chunk whole.
// A chunk takes new shapes only while no packet holds it, and a chunk that
// no item names any more takes new shapes again once no packet holds it.
// A chunk whose next shape would grow it, or that is full, first drops the
// shapes no item draws once they are a quarter of it: a background drawn
// anew each frame leaves its last shapes behind.

use std::{hash::Hasher, num::NonZeroU32, ops::Range, sync::Arc};

use cranpose_ui_graphics::{DrawPrimitive, FxHasher, Rect, ShapeRecorder};

use super::{Item, ItemKind, SceneStore, slab::Slab};

// The most shapes a chunk takes: a chunk that one item still names keeps
// the room of this many.
const CHUNK_SHAPES: usize = 256;
// The most chunks no item names that the store keeps for reuse.
// A packet in flight holds the chunks it draws, so a frame that redraws a
// shape writes into an empty chunk that the packets of the frames before
// let go of.
const KEPT_EMPTY_CHUNKS: usize = 4;

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
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeRun {
    chunk: ChunkId,
    // How many shapes the item draws, and the rect they cover when it
    // draws any: a collect reads them for every item it draws.
    shapes: u32,
    coverage: Rect,
    segments: Range<u32>,
    // A hash of what the shapes draw, which the chunk's other shapes leave
    // alone.
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
}

impl ShapeChunks {
    // Records the shapes of one item with `record` into a chunk that takes
    // them, and returns where they went.
    fn record(
        &mut self,
        runs: &mut Slab<ShapeRun>,
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
        let segments = start..recorder.all_segments().end;
        let shapes = recorder.shape_count_in(&segments);
        entry.items += 1;
        entry.shapes += shapes;
        let mut print = FxHasher::default();
        recorder.hash_in(&segments, &mut print);
        ShapeRun {
            chunk,
            shapes,
            coverage: recorder.coverage_in(&segments).unwrap_or_default(),
            segments,
            print: print.finish(),
        }
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
            if (full || held >= recorder.shape_capacity()) && dead > 0 && dead * 4 >= held {
                keep_live_runs(open, recorder, runs, &mut self.live);
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

    // Notes that the item of `run` went. A chunk no item names waits for
    // reuse; past the few kept, it goes.
    fn release(&mut self, run: &ShapeRun) {
        let chunk = run.chunk;
        let Some(entry) = self.chunks[chunk.index()].as_mut() else {
            return;
        };
        entry.items -= 1;
        entry.shapes -= run.shapes;
        if entry.items > 0 {
            return;
        }
        if self.open == Some(chunk) {
            self.open = None;
        }
        if self.empty.len() < KEPT_EMPTY_CHUNKS {
            self.empty.push(chunk);
        } else {
            self.chunks[chunk.index()] = None;
        }
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

// Moves the shapes of the runs in `chunk` to the front of its recorder, in
// record order, and drops the shapes no run names. `live` is room for one
// bit a segment.
fn keep_live_runs(
    chunk: ChunkId,
    recorder: &mut ShapeRecorder,
    runs: &mut Slab<ShapeRun>,
    live: &mut Vec<u64>,
) {
    live.clear();
    live.resize(recorder.all_segments().end.div_ceil(64) as usize, 0);
    for run in runs.values().filter(|run| run.chunk == chunk) {
        for segment in run.segments.clone() {
            live[segment as usize / 64] |= 1 << (segment % 64);
        }
    }
    recorder.keep_segments(|segment| live[segment as usize / 64] & (1 << (segment % 64)) != 0);
    // A run now starts past the live segments before it.
    for run in runs.values_mut().filter(|run| run.chunk == chunk) {
        let (word, bit) = (run.segments.start as usize / 64, run.segments.start % 64);
        let start = live[..word]
            .iter()
            .map(|bits| bits.count_ones())
            .sum::<u32>()
            + live
                .get(word)
                .map_or(0, |bits| (bits & ((1 << bit) - 1)).count_ones());
        run.segments = start..start + run.segments.len() as u32;
    }
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl SceneStore {
    /// Adds an item of the shapes `record` records into a chunk the store
    /// shares among its shape items.
    pub fn add_shapes(&mut self, record: impl FnOnce(&mut ShapeRecorder)) -> Item {
        let run = self.shape_chunks.record(&mut self.shape_runs, record);
        Item::new(ItemKind::Shape, self.shape_runs.insert(run))
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

    // Removes the shape item at `index` of the shape runs.
    pub(super) fn remove_shapes(&mut self, index: usize) {
        if let Some(run) = self.shape_runs.get(index) {
            self.shape_chunks.release(run);
            self.shape_runs.remove(index);
        }
    }

    // The heap the shape items and their chunks hold.
    pub(super) fn shapes_heap_bytes(&self) -> usize {
        self.shape_runs.heap_bytes() + self.shape_chunks.heap_bytes()
    }

    // How many shape items the store holds.
    pub(super) fn shape_item_count(&self) -> usize {
        self.shape_runs.values().count()
    }

    // Lets go of every shape item and chunk.
    pub(super) fn clear_shapes(&mut self) {
        self.shape_runs.clear();
        self.shape_chunks = ShapeChunks::default();
    }
}
