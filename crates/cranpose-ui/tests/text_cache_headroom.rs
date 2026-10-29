use std::rc::Rc;

use cranpose_ui::{
    AppContext, MemoryHeadroom, TextLayoutOptions, TextStyle, prepare_text_layout,
    set_memory_headroom, text::AnnotatedString,
};

// Lays "kept" out, lets `idle_passes` passes lay out only another text and
// lays "kept" out again in the pass after, on a host that reports
// `headroom`: whether the second layout is the first.
fn kept_after(headroom: MemoryHeadroom, idle_passes: usize) -> bool {
    let kept = AnnotatedString::from("kept");
    let other = AnnotatedString::from("other");
    let style = TextStyle::default();
    let prepare = |text| prepare_text_layout(text, &style, TextLayoutOptions::default(), None);
    let context = AppContext::new();
    context.enter(|| {
        set_memory_headroom(headroom);
        let first = prepare(&kept);
        for _ in 0..idle_passes {
            context.start_text_frame();
            prepare(&other);
        }
        context.start_text_frame();
        Rc::ptr_eq(&first, &prepare(&kept))
    })
}

#[test]
fn a_layout_idle_for_a_pass_stays_with_memory_to_spare() {
    assert!(kept_after(MemoryHeadroom::Ample, 1));
}

#[test]
fn a_tight_heap_keeps_a_layout_the_last_pass_asked_for() {
    assert!(kept_after(MemoryHeadroom::Tight, 0));
}

#[test]
fn a_tight_heap_drops_a_layout_idle_for_a_pass() {
    assert!(!kept_after(MemoryHeadroom::Tight, 1));
}
