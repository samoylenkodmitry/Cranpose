use cranpose_ui::*;

#[test]
fn test_modifier_chain_length_after_updates() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut composition = run_test_composition(|| {
        Box(Modifier::empty().padding(12.0), BoxSpec::default(), || {});
    });

    assert!(
        composition.root().is_some(),
        "Should have root after first render"
    );

    composition = run_test_composition(|| {
        Box(Modifier::empty().padding(12.0), BoxSpec::default(), || {});
    });

    assert!(
        composition.root().is_some(),
        "Should have root after second render"
    );
}

#[test]
fn test_modifier_value_changes_propagate_to_layout() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);

    let run_with_padding = |p: f32| {
        let mut composition = run_test_composition(|| {
            Box(Modifier::empty().padding(p), BoxSpec::default(), || {});
        });

        let root = composition.root().expect("has root");
        let mut applier = composition.applier_mut();
        let layout = applier
            .compute_layout(
                root,
                Size {
                    width: 800.0,
                    height: 600.0,
                },
            )
            .expect("layout computation");

        layout.root().rect.width
    };

    let width1 = run_with_padding(10.0);
    assert_eq!(width1, 20.0, "Width should be 10*2 padding");

    let width2 = run_with_padding(20.0);
    assert_eq!(width2, 40.0, "Width should be 20*2 padding after update");
}

#[test]
fn test_modifier_chain_adds_node() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut composition = run_test_composition(|| {
        Box(Modifier::empty().padding(8.0), BoxSpec::default(), || {});
    });

    assert!(
        composition.root().is_some(),
        "Should have root with padding only"
    );

    composition = run_test_composition(|| {
        Box(
            Modifier::empty()
                .padding(8.0)
                .background(Color(1.0, 0.0, 0.0, 1.0)),
            BoxSpec::default(),
            || {},
        );
    });

    assert!(
        composition.root().is_some(),
        "Should have root with padding + background"
    );
}

#[test]
fn test_modifier_chain_removes_node() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut composition = run_test_composition(|| {
        Box(
            Modifier::empty()
                .padding(8.0)
                .background(Color(1.0, 0.0, 0.0, 1.0)),
            BoxSpec::default(),
            || {},
        );
    });

    assert!(
        composition.root().is_some(),
        "Should have root with padding + background"
    );

    composition = run_test_composition(|| {
        Box(Modifier::empty().padding(8.0), BoxSpec::default(), || {});
    });

    assert!(
        composition.root().is_some(),
        "Should have root with padding only"
    );
}

#[test]
fn test_modifier_order_change() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut composition = run_test_composition(|| {
        Box(
            Modifier::empty().padding(10.0).size(Size {
                width: 100.0,
                height: 100.0,
            }),
            BoxSpec::default(),
            || {},
        );
    });

    let root = composition.root().expect("has root");
    let mut applier = composition.applier_mut();
    let layout1 = applier
        .compute_layout(
            root,
            Size {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("layout computation");

    assert_eq!(
        layout1.root().rect.width,
        120.0,
        "Width should be padding (20) + size (100)"
    );
    drop(applier);

    composition = run_test_composition(|| {
        Box(
            Modifier::empty()
                .size(Size {
                    width: 100.0,
                    height: 100.0,
                })
                .padding(10.0),
            BoxSpec::default(),
            || {},
        );
    });

    let root = composition.root().expect("has root");
    let mut applier = composition.applier_mut();
    let layout2 = applier
        .compute_layout(
            root,
            Size {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("layout computation");

    assert_eq!(
        layout2.root().rect.width,
        100.0,
        "Width should be 100 when size constraint is applied after padding"
    );
}

#[test]
fn test_complex_modifier_chain_updates() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let build_modifier = |step: i32| match step {
        0 => Modifier::empty().padding(5.0),
        1 => Modifier::empty().padding(5.0).size(Size {
            width: 50.0,
            height: 50.0,
        }),
        2 => Modifier::empty()
            .padding(5.0)
            .size(Size {
                width: 50.0,
                height: 50.0,
            })
            .background(Color(0.5, 0.5, 0.5, 1.0)),
        3 => Modifier::empty()
            .size(Size {
                width: 50.0,
                height: 50.0,
            })
            .background(Color(0.5, 0.5, 0.5, 1.0)),
        4 => Modifier::empty().background(Color(0.5, 0.5, 0.5, 1.0)),
        _ => Modifier::empty(),
    };

    for step in 0..=5 {
        let composition = run_test_composition(|| {
            Box(build_modifier(step), BoxSpec::default(), || {});
        });

        assert!(
            composition.root().is_some(),
            "Step {}: should have root",
            step
        );
    }
}
