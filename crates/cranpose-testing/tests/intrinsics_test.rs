use cranpose_ui::*;

#[test]
fn intrinsic_size_modifiers_accept_values() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let _width_min = Modifier::empty().width_intrinsic(IntrinsicSize::Min);
    let _width_max = Modifier::empty().width_intrinsic(IntrinsicSize::Max);
    let _height_min = Modifier::empty().height_intrinsic(IntrinsicSize::Min);
    let _height_max = Modifier::empty().height_intrinsic(IntrinsicSize::Max);
}

#[test]
fn intrinsic_size_can_be_combined_with_other_modifiers() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let _combined = Modifier::empty()
        .width_intrinsic(IntrinsicSize::Max)
        .then(Modifier::empty().padding(8.0))
        .then(Modifier::empty().background(Color(1.0, 0.0, 0.0, 1.0)));
}

#[test]
fn equal_width_buttons_api_demonstration() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let composition = run_test_composition(|| {
        Row(Modifier::empty(), RowSpec::default(), || {
            Button(
                Modifier::empty().width_intrinsic(IntrinsicSize::Max),
                ButtonSpec::default(),
                || {},
                || {
                    Text("OK", Modifier::empty(), TextStyle::default());
                },
            );
            Button(
                Modifier::empty().width_intrinsic(IntrinsicSize::Max),
                ButtonSpec::default(),
                || {},
                || {
                    Text("Cancel", Modifier::empty(), TextStyle::default());
                },
            );
            Button(
                Modifier::empty().width_intrinsic(IntrinsicSize::Max),
                ButtonSpec::default(),
                || {},
                || {
                    Text("Apply", Modifier::empty(), TextStyle::default());
                },
            );
        });
    });

    assert!(composition.root().is_some());
}

#[test]
fn column_with_intrinsic_width() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let composition = run_test_composition(|| {
        Column(
            Modifier::empty()
                .width_intrinsic(IntrinsicSize::Max)
                .then(Modifier::empty().background(Color(0.8, 0.8, 0.8, 1.0))),
            ColumnSpec::default(),
            || {
                Text("Short", Modifier::empty(), TextStyle::default());
                Text("Much Longer Text", Modifier::empty(), TextStyle::default());
                Text("Mid", Modifier::empty(), TextStyle::default());
            },
        );
    });

    assert!(composition.root().is_some());
}

#[test]
fn row_with_intrinsic_height() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let composition = run_test_composition(|| {
        Row(
            Modifier::empty()
                .height_intrinsic(IntrinsicSize::Max)
                .then(Modifier::empty().background(Color(0.8, 0.8, 0.8, 1.0))),
            RowSpec::default(),
            || {
                Box(
                    Modifier::empty().size(Size {
                        width: 50.0,
                        height: 30.0,
                    }),
                    BoxSpec::default(),
                    || {},
                );
                Box(
                    Modifier::empty().size(Size {
                        width: 50.0,
                        height: 80.0,
                    }),
                    BoxSpec::default(),
                    || {},
                );
                Box(
                    Modifier::empty().size(Size {
                        width: 50.0,
                        height: 50.0,
                    }),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
    });

    assert!(composition.root().is_some());
}

#[test]
fn min_intrinsic_vs_max_intrinsic() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let comp_min = run_test_composition(|| {
        Column(
            Modifier::empty().width_intrinsic(IntrinsicSize::Min),
            ColumnSpec::default(),
            || {
                Text("Content", Modifier::empty(), TextStyle::default());
            },
        );
    });

    let comp_max = run_test_composition(|| {
        Column(
            Modifier::empty().width_intrinsic(IntrinsicSize::Max),
            ColumnSpec::default(),
            || {
                Text("Content", Modifier::empty(), TextStyle::default());
            },
        );
    });

    assert!(comp_min.root().is_some());
    assert!(comp_max.root().is_some());
}

#[test]
fn intrinsic_size_with_padding() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let composition = run_test_composition(|| {
        Column(
            Modifier::empty()
                .width_intrinsic(IntrinsicSize::Max)
                .then(Modifier::empty().padding(16.0))
                .then(Modifier::empty().background(Color(0.9, 0.9, 0.9, 1.0))),
            ColumnSpec::default(),
            || {
                Text("Button 1", Modifier::empty(), TextStyle::default());
                Text("Button 2 - Longer", Modifier::empty(), TextStyle::default());
            },
        );
    });

    assert!(composition.root().is_some());
}

#[test]
fn nested_intrinsic_sizing() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let composition = run_test_composition(|| {
        Column(Modifier::empty(), ColumnSpec::default(), || {
            Row(
                Modifier::empty().width_intrinsic(IntrinsicSize::Max),
                RowSpec::default(),
                || {
                    Text("Left", Modifier::empty(), TextStyle::default());
                    Text("Right", Modifier::empty(), TextStyle::default());
                },
            );
            Row(
                Modifier::empty().width_intrinsic(IntrinsicSize::Max),
                RowSpec::default(),
                || {
                    Text("A", Modifier::empty(), TextStyle::default());
                    Text("B", Modifier::empty(), TextStyle::default());
                },
            );
        });
    });

    assert!(composition.root().is_some());
}
