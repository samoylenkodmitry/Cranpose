use super::*;
use crate::app::AsyncRuntimeEngine;
use cranpose_core::{location_key, Composition, MemoryApplier, MutableState, NodeError};

mod conditional_text_test;

#[composable]
fn async_runtime_test_content(
    animation: MutableState<AnimationState>,
    stats: MutableState<FrameStats>,
    is_running: MutableState<bool>,
    reset_signal: MutableState<u64>,
) {
    AsyncRuntimeEngine(animation, stats, is_running, reset_signal);

    Column(
        Modifier::empty()
            .padding(32.0)
            .then(Modifier::empty().background(Color(0.10, 0.14, 0.28, 1.0)))
            .then(Modifier::empty().rounded_corners(24.0))
            .then(Modifier::empty().padding(20.0)),
        ColumnSpec::default(),
        {
            move || {
                let animation_snapshot = animation.get();
                let stats_snapshot = stats.get();
                let progress_value = animation_snapshot.progress.clamp(0.0, 1.0);

                Column(
                    Modifier::empty()
                        .fill_max_width()
                        .then(Modifier::empty().padding(8.0))
                        .then(Modifier::empty().background(Color(0.06, 0.10, 0.22, 0.8)))
                        .then(Modifier::empty().rounded_corners(18.0))
                        .then(Modifier::empty().padding(12.0)),
                    ColumnSpec::default(),
                    {
                        move || {
                            Row(
                                Modifier::empty()
                                    .fill_max_width()
                                    .then(Modifier::empty().height(26.0))
                                    .then(Modifier::empty().rounded_corners(13.0))
                                    .then(Modifier::empty().draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(Color(0.12, 0.16, 0.30, 1.0)),
                                            CornerRadii::uniform(13.0),
                                        );
                                    })),
                                RowSpec::default(),
                                move || {
                                    if progress_value > 0.0 {
                                        Row(
                                            Modifier::empty()
                                                .fill_max_width_fraction(progress_value)
                                                .then(Modifier::empty().height(26.0))
                                                .then(Modifier::empty().rounded_corners(13.0))
                                                .then(Modifier::empty().draw_behind(|scope| {
                                                    scope.draw_round_rect(
                                                        Brush::linear_gradient(vec![
                                                            Color(0.25, 0.55, 0.95, 1.0),
                                                            Color(0.15, 0.35, 0.80, 1.0),
                                                        ]),
                                                        CornerRadii::uniform(13.0),
                                                    );
                                                })),
                                            RowSpec::default(),
                                            || {},
                                        );
                                    }
                                },
                            );
                        }
                    },
                );

                Text(
                    format!(
                        "Frames advanced: {} (last frame {:.2} ms, direction: {})",
                        stats_snapshot.frames,
                        stats_snapshot.last_frame_ms,
                        if animation_snapshot.direction >= 0.0 {
                            "forward"
                        } else {
                            "reverse"
                        }
                    ),
                    Modifier::empty()
                        .padding(8.0)
                        .then(Modifier::empty().background(Color(0.18, 0.22, 0.36, 0.6)))
                        .then(Modifier::empty().rounded_corners(14.0)),
                    TextStyle::default(),
                );
            }
        },
    );
}

fn drain_all(composition: &mut Composition<MemoryApplier>) -> Result<(), NodeError> {
    loop {
        if !composition.process_invalid_scopes()? {
            break;
        }
    }
    Ok(())
}

fn app_context_scope() -> (
    std::rc::Rc<cranpose_ui::AppContext>,
    cranpose_ui::AppContextScope,
) {
    let context = cranpose_ui::AppContext::new();
    let scope = context.enter_scope();
    (context, scope)
}

#[test]
fn async_runtime_freezes_without_conditional_key() {
    let (_app_context, _app_context_scope) = app_context_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();

    let animation = MutableState::with_runtime(AnimationState::default(), runtime.clone());
    let stats = MutableState::with_runtime(FrameStats::default(), runtime.clone());
    let is_running = MutableState::with_runtime(true, runtime.clone());
    let reset_signal = MutableState::with_runtime(0u64, runtime.clone());

    let mut render =
        { move || async_runtime_test_content(animation, stats, is_running, reset_signal) };

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("initial render");
    drain_all(&mut composition).expect("initial drain");

    let mut reached_upper_peak = false;
    let mut returned_to_zero = false;
    let mut second_forward_run = false;
    let mut frames_before = None;
    let mut frames_after = None;
    let mut time = 0u64;

    for _ in 0..2400 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        drain_all(&mut composition).expect("drain after frame");

        let anim = animation.value();
        if anim.progress >= 0.95 {
            reached_upper_peak = true;
        }
        if reached_upper_peak && anim.progress <= 0.05 {
            returned_to_zero = true;
        }
        if returned_to_zero && anim.progress >= 0.20 {
            second_forward_run = true;
            frames_before = Some(stats.value().frames);

            for _ in 0..16 {
                time += 16_666_667;
                runtime.drain_frame_callbacks(time);
                drain_all(&mut composition).expect("drain after flip");
            }

            frames_after = Some(stats.value().frames);
            break;
        }
    }

    assert!(
        second_forward_run,
        "did not observe async runtime return to zero and climb forward again"
    );
    let before = frames_before.expect("frames before flip");
    let after = frames_after.expect("frames after flip");

    assert!(
        after > before,
        "frames should continue increasing after forward flip without manual key stabilization (before {before}, after {after})"
    );
}

#[test]
fn async_runtime_tab_content_renders_static_states() {
    let (_app_context, _app_context_scope) = app_context_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();

    let animation_state = MutableState::with_runtime(
        AnimationState {
            progress: 1.0,
            direction: 1.0,
        },
        runtime.clone(),
    );
    let stats_state = MutableState::with_runtime(
        FrameStats {
            frames: 42,
            last_frame_ms: 16.0,
        },
        runtime.clone(),
    );
    let is_running_state = MutableState::with_runtime(false, runtime.clone());
    let reset_signal_state = MutableState::with_runtime(0u64, runtime);

    let mut render = move || {
        AsyncRuntimeTabContent(
            animation_state,
            stats_state,
            is_running_state,
            reset_signal_state,
        )
    };

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("initial render");
    drain_all(&mut composition).expect("initial drain");

    animation_state.update(|state| state.progress = 0.0);
    drain_all(&mut composition).expect("drain after progress 0");
    animation_state.update(|state| state.progress = 1.0);
    drain_all(&mut composition).expect("drain after progress 1");
}

#[test]
fn async_runtime_engine_stable_progress_does_not_self_invalidate() {
    let (_app_context, _app_context_scope) = app_context_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();

    let animation = MutableState::with_runtime(AnimationState::default(), runtime.clone());
    let stats = MutableState::with_runtime(FrameStats::default(), runtime.clone());
    let is_running = MutableState::with_runtime(false, runtime.clone());
    let reset_signal = MutableState::with_runtime(0u64, runtime);

    let mut render =
        { move || async_runtime_test_content(animation, stats, is_running, reset_signal) };

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("initial render");

    for attempt in 0..4 {
        assert!(
            !composition
                .process_invalid_scopes()
                .expect("stable progress recomposition check"),
            "stable async runtime render self-invalidated on pass {attempt}",
        );
    }
}

#[test]
fn async_runtime_test_content_does_not_grow_slots_across_cycle() {
    let (_app_context, _app_context_scope) = app_context_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();

    let animation = MutableState::with_runtime(AnimationState::default(), runtime.clone());
    let stats = MutableState::with_runtime(FrameStats::default(), runtime.clone());
    let is_running = MutableState::with_runtime(true, runtime.clone());
    let reset_signal = MutableState::with_runtime(0u64, runtime.clone());

    let mut render =
        { move || async_runtime_test_content(animation, stats, is_running, reset_signal) };

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("initial render");
    drain_all(&mut composition).expect("initial drain");

    let baseline_slots = composition.debug_dump_slot_entries().len();
    let mut time = 0u64;
    let mut reached_upper_peak = false;
    let mut returned_to_zero = false;

    for _ in 0..2400 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        drain_all(&mut composition).expect("drain after frame");

        let progress = animation.value().progress;
        if progress >= 0.95 {
            reached_upper_peak = true;
        }
        if reached_upper_peak && progress <= 0.05 {
            returned_to_zero = true;
        }
        if returned_to_zero && progress >= 0.95 {
            break;
        }
    }

    let final_slots = composition.debug_dump_slot_entries();
    assert!(
        final_slots.len() <= baseline_slots + 64,
        "async runtime test content leaked slots: baseline={} final={} slots={final_slots:#?}",
        baseline_slots,
        final_slots.len(),
    );
}

#[test]
fn scrollable_async_runtime_does_not_grow_slots_across_cycle() {
    let (_app_context, _app_context_scope) = app_context_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();

    let animation = MutableState::with_runtime(AnimationState::default(), runtime.clone());
    let stats = MutableState::with_runtime(FrameStats::default(), runtime.clone());
    let is_running = MutableState::with_runtime(true, runtime.clone());
    let reset_signal = MutableState::with_runtime(0u64, runtime.clone());

    composition
        .render(location_key(file!(), line!(), column!()), &mut || {
            ScrollableTab(move || {
                async_runtime_test_content(animation, stats, is_running, reset_signal);
            });
        })
        .expect("initial render");
    drain_all(&mut composition).expect("initial drain");

    let baseline_slots = composition.debug_dump_slot_entries().len();
    let mut time = 0u64;
    let mut reached_upper_peak = false;
    let mut returned_to_zero = false;

    for _ in 0..2400 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        drain_all(&mut composition).expect("drain after frame");

        let progress = animation.value().progress;
        if progress >= 0.95 {
            reached_upper_peak = true;
        }
        if reached_upper_peak && progress <= 0.05 {
            returned_to_zero = true;
        }
        if returned_to_zero && progress >= 0.95 {
            break;
        }
    }

    let final_slots = composition.debug_dump_slot_entries();
    assert!(
        final_slots.len() <= baseline_slots + 64,
        "scrollable async runtime leaked slots: baseline={} final={} slots={final_slots:#?}",
        baseline_slots,
        final_slots.len(),
    );
}

#[test]
fn combined_app_starting_on_async_does_not_grow_slots_across_cycle() {
    let (_app_context, _app_context_scope) = app_context_scope();
    TEST_ACTIVE_TAB_STATE.with(|cell| cell.borrow_mut().take());

    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), &mut || {
            combined_app_with_initial_tab(Some(DemoTab::Async));
        })
        .expect("initial render");
    drain_all(&mut composition).expect("initial drain");

    let runtime = composition.runtime_handle();
    let baseline_slots = composition.debug_dump_slot_entries().len();
    let mut time = 0u64;

    for _ in 0..240 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        drain_all(&mut composition).expect("drain after frame");
    }

    let final_slots = composition.debug_dump_slot_entries();
    assert!(
        final_slots.len() <= baseline_slots + 64,
        "combined app starting on async leaked slots: baseline={} final={} slots={final_slots:#?}",
        baseline_slots,
        final_slots.len(),
    );
}

#[test]
fn lazy_list_tab_uses_internal_scroll_container() {
    assert!(
        !tab_requires_scroll(DemoTab::LazyList),
        "Lazy List tab owns an internal lazy list and must not be wrapped in ScrollableTab"
    );
}

#[test]
fn markdown_tab_uses_internal_scroll_container() {
    assert!(
        !tab_requires_scroll(DemoTab::MarkdownViewer),
        "Markdown tab owns an internal lazy list + scrollbar and must not be wrapped in ScrollableTab"
    );
}

#[test]
fn startup_tab_parser_accepts_variant_and_label_aliases() {
    assert_eq!(
        DemoTab::from_startup_name("shaders"),
        Some(DemoTab::Shaders)
    );
    assert_eq!(
        DemoTab::from_startup_name("Recursive Layout"),
        Some(DemoTab::Layout)
    );
    assert_eq!(
        DemoTab::from_startup_name("modifier_showcase"),
        Some(DemoTab::ModifierShowcase)
    );
    assert_eq!(
        DemoTab::from_startup_name("markdown-viewer"),
        Some(DemoTab::MarkdownViewer)
    );
}

#[test]
fn startup_tab_parser_rejects_unknown_names() {
    assert_eq!(DemoTab::from_startup_name(""), None);
    assert_eq!(DemoTab::from_startup_name("definitely-not-a-tab"), None);
}

#[test]
fn startup_selection_focuses_shaders_when_only_section_is_requested() {
    let startup = StartupSelection::from_requested(None, Some(ShaderSection::EffectSemantics));
    assert_eq!(startup.initial_tab, Some(DemoTab::Shaders));
    assert_eq!(
        startup.initial_shader_section,
        Some(ShaderSection::EffectSemantics)
    );
}

#[test]
fn startup_selection_ignores_shader_section_for_non_shader_tab() {
    let startup = StartupSelection::from_requested(
        Some(DemoTab::Counter),
        Some(ShaderSection::InteractiveEffects),
    );
    assert_eq!(startup.initial_tab, Some(DemoTab::Counter));
    assert_eq!(startup.initial_shader_section, None);
}

#[test]
fn startup_selection_keeps_shader_section_for_shaders_tab() {
    let startup =
        StartupSelection::from_requested(Some(DemoTab::Shaders), Some(ShaderSection::MaskApi));
    assert_eq!(startup.initial_tab, Some(DemoTab::Shaders));
    assert_eq!(startup.initial_shader_section, Some(ShaderSection::MaskApi));
}

/// The Wear tab composes a real widget tree, not a canvas.
///
/// It replaced a vendored copy of an app's own list, draw, font, layout and
/// theme stack that drew the same screen through a single `Canvas`. This is the
/// check that the widgets it replaced that copy with actually lay out: a round
/// screen filling its box, a scaling list inside it, and rows placed down the
/// middle rather than stacked at the origin.
#[test]
fn the_wear_tab_lays_out_a_watch_screen_of_real_widgets() {
    use crate::app::wear::wear_tab;
    use cranpose_ui::{measure_layout, run_test_composition};

    let mut composition = run_test_composition(|| {
        cranpose_ui::set_density(2.0);
        wear_tab();
    });
    let root = composition.root().expect("wear tab root");
    let handle = composition.runtime_handle();
    let tree = {
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle);
        let tree = measure_layout(
            &mut applier,
            root,
            Size {
                width: 600.0,
                height: 600.0,
            },
        )
        .expect("layout measurement")
        .into_layout_tree()
        .expect("layout tree");
        applier.clear_runtime_handle();
        tree
    };

    fn rows(node: &cranpose_ui::LayoutBox, out: &mut Vec<cranpose_ui::Rect>) {
        if node.node_data.modifier_slices().graphics_layer().is_some() {
            out.push(node.rect);
        }
        for child in &node.children {
            rows(child, out);
        }
    }
    let mut placed = Vec::new();
    rows(tree.root(), &mut placed);
    assert!(
        placed.len() >= 15,
        "every row of the settings screen is composed: {}",
        placed.len()
    );
    // The rows run down the screen in order and none of them is left at the
    // origin, which is what an unplaced node would look like.
    for pair in placed.windows(2) {
        assert!(pair[1].y > pair[0].y, "{:?}", placed);
    }
    assert!(placed
        .iter()
        .all(|rect| rect.width > 0.0 && rect.height > 0.0));
}
