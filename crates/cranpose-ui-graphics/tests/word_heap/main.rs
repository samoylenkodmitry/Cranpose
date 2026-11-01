// The classic ESP32's shape columns, which `cfg(word_heap)` builds: run with
// `RUSTFLAGS="--cfg word_heap"`, as `just test-word-heap` does. Its own
// binary: the word heap is the process's once named.
#![cfg(word_heap)]
#![expect(
    unsafe_code,
    reason = "a word heap is an allocator the test implements"
)]

use std::{
    alloc::Layout,
    sync::atomic::{AtomicUsize, Ordering},
};

use cranpose_ui_graphics::{
    Brush, Color, CommandRecording, DrawPrimitive, Rect, WordHeap, set_word_heap,
    set_word_heap_open,
};

static ROOM: AtomicUsize = AtomicUsize::new(0);
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);
static FREED: AtomicUsize = AtomicUsize::new(0);

// A heap of `ROOM` words that keeps each block's word count in the word
// before it.
unsafe fn take_words(words: usize) -> *mut u32 {
    if ROOM
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |room| {
            room.checked_sub(words)
        })
        .is_err()
    {
        return std::ptr::null_mut();
    }
    let layout = Layout::array::<u32>(words + 1).expect("a small block");
    // SAFETY: the layout holds at least one word.
    let block = unsafe { std::alloc::alloc(layout) }.cast::<u32>();
    assert!(!block.is_null(), "the test heap's block");
    // SAFETY: the block holds `words + 1` words.
    unsafe {
        block.write(u32::try_from(words).expect("a small block"));
        ALLOCATED.fetch_add(1, Ordering::SeqCst);
        block.add(1)
    }
}

unsafe fn give_words(words: *mut u32) {
    // SAFETY: `take_words` returned `words`, one word past its block's start.
    unsafe {
        let block = words.sub(1);
        let count = block.read() as usize;
        ROOM.fetch_add(count, Ordering::SeqCst);
        std::alloc::dealloc(
            block.cast(),
            Layout::array::<u32>(count + 1).expect("a small block"),
        );
    }
    FREED.fetch_add(1, Ordering::SeqCst);
}

fn rects(recording: &mut CommandRecording, count: usize, y: f32) {
    let mut recorder = recording.record();
    for index in 0..count {
        recorder.push_primitive(DrawPrimitive::Rect {
            rect: Rect {
                x: index as f32,
                y,
                width: 3.0,
                height: 4.0,
            },
            brush: Brush::Solid(Color::RED),
            stroke: None,
        });
    }
    recorder.finish();
}

// A recording's shape columns take the word heap while it is open and has
// room and the global allocator otherwise, read, compare and hash the same
// from either, and give the word heap's blocks back when the recording
// drops.
#[test]
fn shape_columns_take_the_word_heap_while_it_has_room() {
    assert!(set_word_heap(WordHeap {
        alloc: take_words,
        free: give_words,
    }));
    // Room for the columns of eight shapes, 16 and 8 words a shape: a first
    // push makes room for four.
    ROOM.store(8 * (16 + 8), Ordering::SeqCst);
    let mut closed = CommandRecording::default();
    rects(&mut closed, 3, 2.0);
    assert_eq!(
        ALLOCATED.load(Ordering::SeqCst),
        0,
        "a closed heap took columns"
    );
    set_word_heap_open(true);
    let [mut single, mut held] = [CommandRecording::default(), CommandRecording::default()];
    rects(&mut single, 1, 2.0);
    rects(&mut held, 3, 2.0);
    let taken = ALLOCATED.load(Ordering::SeqCst);
    assert!(taken > 0, "the columns took the word heap");
    // The rest of the room goes, as to other columns.
    ROOM.store(0, Ordering::SeqCst);
    let [mut single_spilled, mut held_spilled, mut grown] = [
        CommandRecording::default(),
        CommandRecording::default(),
        CommandRecording::default(),
    ];
    rects(&mut single_spilled, 1, 2.0);
    rects(&mut held_spilled, 3, 2.0);
    rects(&mut grown, 40, 2.0);
    assert_eq!(
        ALLOCATED.load(Ordering::SeqCst),
        taken,
        "no room took no block"
    );
    for (recording, count) in [(&held, 3), (&grown, 40)] {
        let shapes = recording.shapes();
        assert_eq!(shapes.len(), count);
        for (index, shape) in shapes.iter().enumerate() {
            assert_eq!(shape.rect, [index as f32, 2.0, 3.0, 4.0]);
        }
        assert_eq!(shapes.get(count - 1), shapes.iter().last());
    }
    // One shape's curves hash by pairs of words, three shapes' columns in
    // four lanes.
    for (word_heap, global) in [
        (&single, &single_spilled),
        (&held, &held_spilled),
        (&held, &closed),
    ] {
        assert!(word_heap.shapes() == global.shapes());
        assert_eq!(word_heap.fingerprint(), global.fingerprint());
    }
    let copy = held.clone();
    assert!(copy.shapes() == held.shapes());
    assert_eq!(copy.fingerprint(), held.fingerprint());
    let mut moved = CommandRecording::default();
    rects(&mut moved, 3, 2.5);
    assert!(moved.shapes() != held.shapes());
    assert_ne!(moved.fingerprint(), held.fingerprint());
    drop((
        closed,
        single,
        held,
        single_spilled,
        held_spilled,
        grown,
        copy,
        moved,
    ));
    assert_eq!(
        FREED.load(Ordering::SeqCst),
        ALLOCATED.load(Ordering::SeqCst),
        "every word heap block went back"
    );
}
