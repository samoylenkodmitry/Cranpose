use std::sync::{Arc, mpsc};

use super::*;
use crate::text::{AnnotatedString, TextLayoutResult, TextMeasurer, TextMetrics, TextStyle};

#[test]
fn a_draw_closure_can_name_its_own_node_for_the_next_frame() {
    let _scope = app_context_test_scope();
    let _ = take_draw_repass_nodes();
    let _ = take_render_invalidation();

    observe_draw_reads(DrawObservationScope::new(33, 0), || {
        request_current_draw_redraw();
    });
    assert_eq!(
        take_draw_repass_nodes(),
        vec![33],
        "the recording node must be scheduled for re-record"
    );
    assert!(
        take_render_invalidation(),
        "the next frame must be requested"
    );

    request_current_draw_redraw();
    assert!(take_draw_repass_nodes().is_empty());
    assert!(take_render_invalidation());
}

struct TestTextMeasurer;

impl TextMeasurer for TestTextMeasurer {
    fn measure(&self, text: &AnnotatedString, _style: &TextStyle) -> TextMetrics {
        TextMetrics {
            width: text.text.len() as f32,
            height: 1.0,
            line_height: 1.0,
            line_count: 1,
        }
    }

    fn get_offset_for_position(
        &self,
        text: &AnnotatedString,
        _style: &TextStyle,
        x: f32,
        _y: f32,
    ) -> usize {
        x.round().max(0.0) as usize % text.text.len().max(1)
    }

    fn get_cursor_x_for_offset(
        &self,
        _text: &AnnotatedString,
        _style: &TextStyle,
        offset: usize,
    ) -> f32 {
        offset as f32
    }

    fn layout(&self, text: &AnnotatedString, _style: &TextStyle) -> TextLayoutResult {
        TextLayoutResult::monospaced(&text.text, 1.0, 1.0)
    }
}

#[test]
fn app_context_ids_do_not_use_process_global_counter() {
    let source = include_str!("../render_state.rs");
    assert!(!source.contains(concat!("NEXT_", "APP_CONTEXT_ID: Atomic")));
}

#[test]
fn app_context_ids_are_unique_within_thread_registry() {
    let first = AppContext::new();
    let second = AppContext::new();

    assert_ne!(first.id, second.id);
    assert!(app_context_by_id(first.id).is_some());
    assert!(app_context_by_id(second.id).is_some());
}

#[test]
fn set_text_measurer_requires_active_app_context() {
    let result = std::panic::catch_unwind(|| {
        crate::text::set_text_measurer(TestTextMeasurer);
    });
    assert!(result.is_err());

    let context = AppContext::new();
    context.enter(|| {
        crate::text::set_text_measurer(TestTextMeasurer);
    });
}

#[test]
fn the_font_scale_starts_at_one_and_invalidates_layout_when_it_moves() {
    let context = AppContext::new();
    context.enter(|| {
        assert_eq!(current_font_scale(), 1.0);
        let _ = take_layout_invalidation();

        set_font_scale(1.3);
        assert_eq!(current_font_scale(), 1.3);
        assert!(
            take_layout_invalidation(),
            "every Sp on screen just changed size"
        );

        set_font_scale(1.3);
        assert!(!take_layout_invalidation());
    });
}

#[test]
fn a_font_scale_no_platform_reports_is_refused() {
    let context = AppContext::new();
    context.enter(|| {
        for nonsense in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            set_font_scale(1.0);
            set_font_scale(nonsense);
            assert_eq!(current_font_scale(), 1.0, "{nonsense} was let through");
        }
        set_font_scale(99.0);
        assert_eq!(current_font_scale(), MAX_FONT_SCALE);
        set_font_scale(0.01);
        assert_eq!(current_font_scale(), MIN_FONT_SCALE);
    });
}

#[test]
fn the_font_scale_is_per_app_context() {
    let first = AppContext::new();
    let second = AppContext::new();
    first.enter(|| set_font_scale(1.5));
    first.enter(|| assert_eq!(current_font_scale(), 1.5));
    second.enter(|| assert_eq!(current_font_scale(), 1.0));
}

#[test]
fn invalidation_flags_are_shared_across_threads() {
    let state = Arc::new(RenderState::new_with_density(1.0));
    let (tx, rx) = mpsc::channel();
    let worker_state = Arc::clone(&state);

    let handle = std::thread::spawn(move || {
        worker_state
            .render_invalidated
            .store(true, Ordering::Relaxed);
        worker_state
            .pointer_invalidated
            .store(true, Ordering::Relaxed);
        worker_state
            .focus_invalidated
            .store(true, Ordering::Relaxed);
        worker_state
            .layout_invalidated
            .store(true, Ordering::Relaxed);
        worker_state
            .density_bits
            .store(f32::to_bits(2.0), Ordering::Relaxed);
        tx.send(()).expect("signal invalidation setup");

        f32::from_bits(worker_state.density_bits.load(Ordering::Relaxed))
    });

    rx.recv().expect("wait for worker invalidation setup");
    assert!(state.render_invalidated.load(Ordering::Relaxed));
    assert!(state.pointer_invalidated.load(Ordering::Relaxed));
    assert!(state.focus_invalidated.load(Ordering::Relaxed));
    assert!(state.layout_invalidated.load(Ordering::Relaxed));
    assert_eq!(
        f32::from_bits(state.density_bits.load(Ordering::Relaxed)),
        2.0
    );
    assert!(state.render_invalidated.swap(false, Ordering::Relaxed));
    assert!(state.pointer_invalidated.swap(false, Ordering::Relaxed));
    assert!(state.focus_invalidated.swap(false, Ordering::Relaxed));
    assert!(state.layout_invalidated.swap(false, Ordering::Relaxed));

    let density = handle.join().expect("worker invalidation snapshot");
    assert_eq!(density, 2.0);
    assert!(!state.render_invalidated.load(Ordering::Relaxed));
    assert!(!state.pointer_invalidated.load(Ordering::Relaxed));
    assert!(!state.focus_invalidated.load(Ordering::Relaxed));
    assert!(!state.layout_invalidated.load(Ordering::Relaxed));
}

#[test]
fn app_contexts_keep_density_and_invalidations_isolated() {
    let first = AppContext::new_with_density(1.0);
    let second = AppContext::new_with_density(1.0);

    first.enter(|| {
        set_density(2.0);
        request_render_invalidation();
        request_pointer_invalidation();
        schedule_layout_repass(11);
        schedule_draw_repass(12);
    });

    second.enter(|| {
        assert_eq!(current_density(), 1.0);
        assert!(!peek_render_invalidation());
        assert!(!peek_pointer_invalidation());
        assert!(!peek_layout_invalidation());
        assert!(!has_pending_layout_repasses());
        assert!(!has_pending_draw_repasses());
    });

    first.enter(|| {
        assert_eq!(current_density(), 2.0);
        assert!(peek_render_invalidation());
        assert!(peek_pointer_invalidation());
        assert!(peek_layout_invalidation());
        assert!(has_pending_layout_repasses());
        assert!(has_pending_draw_repasses());
        assert_eq!(take_layout_repass_nodes(), vec![11]);
        assert_eq!(take_draw_repass_nodes(), vec![12]);
        assert!(take_render_invalidation());
        assert!(take_pointer_invalidation());
        assert!(take_layout_invalidation());
    });
}

#[test]
fn app_contexts_keep_fling_velocity_diagnostics_isolated() {
    let first = AppContext::new_with_density(1.0);
    let second = AppContext::new_with_density(1.0);

    first.enter(|| {
        record_last_fling_velocity(1200.0);
        assert_eq!(debug_last_fling_velocity(), 1200.0);
    });

    second.enter(|| {
        assert_eq!(debug_last_fling_velocity(), 0.0);
        record_last_fling_velocity(-450.0);
        assert_eq!(debug_last_fling_velocity(), -450.0);
    });

    first.enter(|| {
        assert_eq!(debug_last_fling_velocity(), 1200.0);
        debug_reset_last_fling_velocity();
        assert_eq!(debug_last_fling_velocity(), 0.0);
    });

    second.enter(|| {
        assert_eq!(debug_last_fling_velocity(), -450.0);
    });
}

#[test]
fn app_context_new_uses_independent_density() {
    let outer = AppContext::new_with_density(2.0);
    let context = AppContext::new();
    context.enter(|| {
        assert_eq!(current_density(), 1.0);
    });
    outer.enter(|| {
        assert_eq!(current_density(), 2.0);
    });
}

#[test]
fn runtime_state_access_requires_explicit_app_context_even_in_tests() {
    let result = std::panic::catch_unwind(|| {
        request_render_invalidation();
    });
    assert!(result.is_err());
}

#[test]
fn app_contexts_keep_layout_frame_arenas_isolated() {
    let first = AppContext::new_with_density(1.0);
    let second = AppContext::new_with_density(1.0);

    first.enter(|| {
        assert_eq!(layout_frame_arena_placement_scratch_count(), 0);
        let mut arena = take_layout_frame_arena();
        arena.seed_placement_scratch_for_test();
        replace_layout_frame_arena(arena);
        assert_eq!(layout_frame_arena_placement_scratch_count(), 1);
    });

    second.enter(|| {
        assert_eq!(layout_frame_arena_placement_scratch_count(), 0);
    });

    first.enter(|| {
        assert_eq!(layout_frame_arena_placement_scratch_count(), 1);
    });
}

#[test]
fn current_app_context_scope_does_not_extend_context_lifetime() {
    let weak = {
        let context = AppContext::new_with_density(1.0);
        let weak = Rc::downgrade(&context);
        context.enter(|| {
            assert!(current_app_context().is_some());
        });
        weak
    };

    assert!(weak.upgrade().is_none());
    assert!(current_app_context().is_none());
}

#[test]
fn dropped_app_context_unregisters_from_thread_lookup_registry() {
    let start_count = app_context_registry_entry_count();

    let id = {
        let context = AppContext::new_with_density(1.0);
        let id = context.id;
        assert!(app_context_by_id(id).is_some());
        id
    };

    assert_eq!(
        app_context_registry_entry_count(),
        start_count,
        "dropped AppContexts must remove their weak registry entry"
    );
    assert!(app_context_by_id(id).is_none());
}
