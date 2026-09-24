use std::sync::Arc;

use cranpose_core::{Composition, DefaultScheduler, MemoryApplier, Runtime, location_key};

use super::*;

fn with_test_runtime<T>(f: impl FnOnce() -> T) -> T {
    let _runtime = Runtime::new(Arc::new(DefaultScheduler));
    f()
}

#[test]
fn circular_arc_path_parses_and_stays_in_bounds() {
    for rotation in [0.0_f32, 45.0, 90.0, 200.0, 355.0] {
        for sweep in [MIN_SWEEP_DEGREES, 120.0, MAX_SWEEP_DEGREES] {
            let data = circular_arc_path_data(
                CIRCULAR_INDICATOR_DIAMETER,
                CIRCULAR_INDICATOR_DIAMETER,
                CIRCULAR_INDICATOR_STROKE_WIDTH,
                rotation - 90.0,
                sweep,
            )
            .expect("arc path data");
            let path = VectorPath::parse(&data).expect("valid SVG arc path");
            assert!(!path.is_empty(), "arc path must produce geometry");
            let bounds = path.bounds();
            let eps = 0.51;
            assert!(
                bounds.x >= -eps
                    && bounds.y >= -eps
                    && bounds.x + bounds.width <= CIRCULAR_INDICATOR_DIAMETER + eps
                    && bounds.y + bounds.height <= CIRCULAR_INDICATOR_DIAMETER + eps,
                "arc (rotation {rotation}, sweep {sweep}) escapes indicator bounds: {bounds:?}"
            );
        }
    }
}

#[test]
fn circular_arc_path_rotates_with_angle() {
    let at =
        |start: f32| circular_arc_path_data(20.0, 20.0, 2.0, start, 120.0).expect("arc path data");
    assert_ne!(at(0.0), at(90.0), "rotation must move the arc");
}

#[test]
fn circular_arc_path_rejects_degenerate_input() {
    assert!(circular_arc_path_data(0.0, 0.0, 2.0, 0.0, 120.0).is_none());
    assert!(circular_arc_path_data(20.0, 20.0, 2.0, 0.0, 0.0).is_none());
}

#[test]
fn linear_band_stays_inside_track() {
    let width = 200.0;
    let mut seen_band = false;
    for step in 0..=20 {
        let phase = step as f32 / 20.0;
        if let Some((x, band_width)) = linear_indicator_band(width, phase) {
            seen_band = true;
            assert!(x >= 0.0, "band start below 0 at phase {phase}");
            assert!(
                x + band_width <= width + 1e-3,
                "band escapes track at phase {phase}"
            );
            assert!(band_width > 0.0);
        }
    }
    assert!(seen_band, "band must be visible for mid phases");
    assert!(linear_indicator_band(width, 0.0).is_none());
    assert!(linear_indicator_band(width, 1.0).is_none());
}

#[test]
fn circular_progress_indicator_composes() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let mut composition = Composition::new(MemoryApplier::new());
        let result = composition.render(location_key(file!(), line!(), column!()), || {
            CircularProgressIndicator(
                Modifier::empty(),
                PROGRESS_INDICATOR_COLOR,
                CIRCULAR_INDICATOR_STROKE_WIDTH,
            );
        });
        assert!(result.is_ok());
        assert!(composition.root().is_some());
    });
}

#[test]
fn linear_progress_indicator_composes() {
    let _app_context = crate::render_state::app_context_test_scope();
    with_test_runtime(|| {
        let mut composition = Composition::new(MemoryApplier::new());
        let result = composition.render(location_key(file!(), line!(), column!()), || {
            LinearProgressIndicator(Modifier::empty(), PROGRESS_INDICATOR_COLOR);
        });
        assert!(result.is_ok());
        assert!(composition.root().is_some());
    });
}

#[test]
fn circular_progress_indicator_animates_transition() {
    use crate::{layout::MeasureLayoutOptions, measure_layout_with_options};

    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), || {
            CircularProgressIndicator(
                Modifier::empty(),
                PROGRESS_INDICATOR_COLOR,
                CIRCULAR_INDICATOR_STROKE_WIDTH,
            );
        })
        .expect("initial render");

    let root = composition.root().expect("composition root");
    let handle = composition.runtime_handle();

    fn collect_draw_commands(
        node: &crate::LayoutBox,
        out: &mut Vec<(crate::DrawCommand, crate::modifier::Size)>,
    ) {
        for command in node.node_data.modifier_slices().draw_commands() {
            out.push((
                command.clone(),
                crate::modifier::Size {
                    width: node.rect.width,
                    height: node.rect.height,
                },
            ));
        }
        for child in &node.children {
            collect_draw_commands(child, out);
        }
    }

    let commands = {
        let mut applier = composition.applier_mut();
        applier.set_runtime_handle(handle.clone());
        let measurements = measure_layout_with_options(
            &mut applier,
            root,
            crate::Size::new(200.0, 200.0),
            MeasureLayoutOptions {
                collect_semantics: false,
                build_layout_tree: true,
            },
        )
        .expect("measure spinner layout");
        applier.clear_runtime_handle();

        let tree = measurements.layout_tree().expect("layout tree");
        let mut commands = Vec::new();
        collect_draw_commands(tree.root(), &mut commands);
        commands
    };
    assert!(!commands.is_empty(), "spinner must register draw commands");

    let run_commands = |commands: &[(crate::DrawCommand, crate::modifier::Size)]| {
        use cranpose_ui_graphics::DrawScope as _;
        commands
            .iter()
            .flat_map(|(command, size)| {
                let func = match command {
                    crate::DrawCommand::Behind(func) => func,
                    crate::DrawCommand::Overlay(func) => func,
                    crate::DrawCommand::WithContent(func) => func,
                };
                let mut scope = crate::draw::command_draw_scope(*size);
                func(&mut scope);
                scope.into_primitives()
            })
            .collect::<Vec<_>>()
    };

    let before = run_commands(&commands);
    assert!(
        !before.is_empty(),
        "spinner draw closure must emit primitives"
    );

    let mut time = 0u64;
    for _ in 0..30 {
        time += 16_666_667;
        handle.drain_frame_callbacks(time);
        composition
            .process_invalid_scopes()
            .expect("process invalid scopes");
    }

    let after = run_commands(&commands);
    assert_ne!(
        before, after,
        "spinner draw primitives must change as the transition animates"
    );
}
