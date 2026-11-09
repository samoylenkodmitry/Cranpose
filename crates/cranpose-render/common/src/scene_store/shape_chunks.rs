// The shapes of the draws that are plain data, a background's among them,
// recorded into chunks that the runs of the whole store share: a shape item
// names its chunk and its segments there, and a packet leases a chunk whole.
// A chunk takes new shapes only while no packet holds it, and a chunk that
// no item names any more takes new shapes again once no packet holds it.

use std::{hash::Hasher, ops::Range, sync::Arc};

use cranpose_ui_graphics::{DrawPrimitive, FxHasher, Rect, ShapeRecorder};

use super::{Item, ItemKind, SceneStore};

// The most shapes a chunk takes: a chunk that one item still names keeps
// the room of this many.
const CHUNK_SHAPES: usize = 256;
// The most chunks no item names that the store keeps for reuse.
// A packet in flight holds the chunks it draws, so a frame that redraws a
// shape writes into an empty chunk that the packets of the frames before
// let go of.
const KEPT_EMPTY_CHUNKS: usize = 4;

/// The shapes of one shape item in its chunk.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeRun {
    chunk: u32,
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
    open: Option<u32>,
    // The chunks no item names, which take shapes again once no packet
    // holds them.
    empty: Vec<u32>,
}

#[derive(Clone)]
struct ShapeChunk {
    recorder: Arc<ShapeRecorder>,
    // How many items name the chunk.
    items: u32,
}

impl ShapeChunks {
    // Records the shapes of one item with `record` into a chunk that takes
    // them, and returns where they went.
    fn record(&mut self, record: impl FnOnce(&mut ShapeRecorder)) -> ShapeRun {
        let chunk = self.writable_chunk();
        let entry = self.chunks[chunk as usize]
            .as_mut()
            .expect("a writable chunk is there");
        let recorder = Arc::get_mut(&mut entry.recorder).expect("no packet holds a writable chunk");
        let start = recorder.all_segments().end;
        record(recorder);
        recorder.end_segment();
        entry.items += 1;
        let segments = start..recorder.all_segments().end;
        let mut print = FxHasher::default();
        recorder.hash_in(&segments, &mut print);
        ShapeRun {
            chunk,
            shapes: recorder.shape_count_in(&segments),
            coverage: recorder.coverage_in(&segments).unwrap_or_default(),
            segments,
            print: print.finish(),
        }
    }

    // A chunk with room that no packet holds: the open one, an empty one
    // emptied of its old shapes, or a new one.
    fn writable_chunk(&mut self) -> u32 {
        let takes = |chunk: &mut ShapeChunk| {
            Arc::get_mut(&mut chunk.recorder)
                .is_some_and(|recorder| recorder.tables().shapes.len() < CHUNK_SHAPES)
        };
        if let Some(open) = self.open
            && self.chunks[open as usize].as_mut().is_some_and(takes)
        {
            return open;
        }
        let reused = self.empty.iter().position(|&id| {
            self.chunks[id as usize]
                .as_mut()
                .is_some_and(|chunk| Arc::get_mut(&mut chunk.recorder).is_some())
        });
        let id = match reused {
            Some(at) => {
                let id = self.empty.swap_remove(at);
                if let Some(recorder) = self.chunks[id as usize]
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

    fn add_chunk(&mut self) -> u32 {
        let chunk = Some(ShapeChunk {
            recorder: Arc::default(),
            items: 0,
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
        u32::try_from(id).expect("a store holds fewer than 2^32 shape chunks")
    }

    // Notes that an item that named `chunk` went. A chunk no item names
    // waits for reuse; past the few kept, it goes.
    fn release(&mut self, chunk: u32) {
        let Some(entry) = self.chunks[chunk as usize].as_mut() else {
            return;
        };
        entry.items -= 1;
        if entry.items > 0 {
            return;
        }
        if self.open == Some(chunk) {
            self.open = None;
        }
        if self.empty.len() < KEPT_EMPTY_CHUNKS {
            self.empty.push(chunk);
        } else {
            self.chunks[chunk as usize] = None;
        }
    }

    fn recorder(&self, chunk: u32) -> &Arc<ShapeRecorder> {
        &self.chunks[chunk as usize]
            .as_ref()
            .expect("a shape item names a live chunk")
            .recorder
    }
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl SceneStore {
    /// Adds an item of the shapes `record` records into a chunk the store
    /// shares among its shape items.
    pub fn add_shapes(&mut self, record: impl FnOnce(&mut ShapeRecorder)) -> Item {
        let run = self.shape_chunks.record(record);
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
        if let Some(chunk) = self.shape_runs.get(index).map(|run| run.chunk) {
            self.shape_chunks.release(chunk);
            self.shape_runs.remove(index);
        }
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
