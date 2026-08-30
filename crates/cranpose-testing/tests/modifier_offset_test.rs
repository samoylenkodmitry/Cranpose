use cranpose_ui::*;

#[test]
fn test_padding_affects_child_position() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut composition = run_test_composition(|| {
        Box(Modifier::empty().padding(20.0), BoxSpec::default(), || {
            Box(
                Modifier::empty().size(Size {
                    width: 50.0,
                    height: 50.0,
                }),
                BoxSpec::default(),
                || {},
            );
        });
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

    let outer_rect = &layout.root().rect;
    assert_eq!(outer_rect.x, 0.0, "Outer box should be at x=0");
    assert_eq!(outer_rect.y, 0.0, "Outer box should be at y=0");
    assert_eq!(
        outer_rect.width, 90.0,
        "Outer width should be child (50) + padding (20*2)"
    );
    assert_eq!(
        outer_rect.height, 90.0,
        "Outer height should be child (50) + padding (20*2)"
    );

    assert_eq!(layout.root().children.len(), 1, "Should have one child");
    let child_rect = &layout.root().children[0].rect;
    assert_eq!(
        child_rect.x, 20.0,
        "Child should be offset by padding.left (20)"
    );
    assert_eq!(
        child_rect.y, 20.0,
        "Child should be offset by padding.top (20)"
    );
    assert_eq!(child_rect.width, 50.0, "Child width should be 50");
    assert_eq!(child_rect.height, 50.0, "Child height should be 50");
}

#[test]
fn test_offset_modifier_affects_position() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut composition = run_test_composition(|| {
        Column(Modifier::empty(), ColumnSpec::default(), || {
            Box(
                Modifier::empty().size(Size {
                    width: 50.0,
                    height: 50.0,
                }),
                BoxSpec::default(),
                || {},
            );

            Box(
                Modifier::empty()
                    .size(Size {
                        width: 50.0,
                        height: 50.0,
                    })
                    .offset(30.0, 15.0),
                BoxSpec::default(),
                || {},
            );
        });
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

    assert_eq!(layout.root().children.len(), 2, "Should have two children");

    let first_box = &layout.root().children[0].rect;
    let second_box = &layout.root().children[1].rect;

    assert_eq!(first_box.x, 0.0, "First box should be at x=0");
    assert_eq!(first_box.y, 0.0, "First box should be at y=0");

    assert_eq!(
        second_box.x, 30.0,
        "Second box should be offset by 30 in x (0 + offset.x)"
    );
    assert_eq!(
        second_box.y, 65.0,
        "Second box should be offset by 15 in y (50 from first box + 15 offset)"
    );
}

#[test]
fn test_padding_and_offset_combined() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut composition = run_test_composition(|| {
        Box(
            Modifier::empty().padding(10.0).offset(20.0, 30.0),
            BoxSpec::default(),
            || {
                Box(
                    Modifier::empty().size(Size {
                        width: 40.0,
                        height: 40.0,
                    }),
                    BoxSpec::default(),
                    || {},
                );
            },
        );
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

    let outer_rect = &layout.root().rect;
    assert_eq!(
        outer_rect.x, 20.0,
        "Outer box should be offset by offset.x (20)"
    );
    assert_eq!(
        outer_rect.y, 30.0,
        "Outer box should be offset by offset.y (30)"
    );
    assert_eq!(
        outer_rect.width, 60.0,
        "Outer width should be child (40) + padding (10*2)"
    );
    assert_eq!(
        outer_rect.height, 60.0,
        "Outer height should be child (40) + padding (10*2)"
    );

    assert_eq!(layout.root().children.len(), 1, "Should have one child");
    let child_rect = &layout.root().children[0].rect;
    assert_eq!(
        child_rect.x, 30.0,
        "Child x should be parent offset (20) + padding (10)"
    );
    assert_eq!(
        child_rect.y, 40.0,
        "Child y should be parent offset (30) + padding (10)"
    );
    assert_eq!(child_rect.width, 40.0, "Child width should be 40");
    assert_eq!(child_rect.height, 40.0, "Child height should be 40");
}

#[test]
fn test_no_double_offset_application() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut composition = run_test_composition(|| {
        Column(Modifier::empty(), ColumnSpec::default(), || {
            Box(
                Modifier::empty()
                    .size(Size {
                        width: 100.0,
                        height: 50.0,
                    })
                    .offset(25.0, 10.0),
                BoxSpec::default(),
                || {},
            );

            Box(
                Modifier::empty().size(Size {
                    width: 100.0,
                    height: 50.0,
                }),
                BoxSpec::default(),
                || {},
            );
        });
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

    assert_eq!(layout.root().children.len(), 2, "Should have two children");

    let first_box = &layout.root().children[0].rect;
    let second_box = &layout.root().children[1].rect;

    assert_eq!(
        first_box.x, 25.0,
        "First box offset should be applied exactly once (25, not 50)"
    );
    assert_eq!(
        first_box.y, 10.0,
        "First box offset should be applied exactly once (10, not 20)"
    );

    assert_eq!(second_box.x, 0.0, "Second box should be at x=0");
    assert_eq!(
        second_box.y, 50.0,
        "Second box should be at y=50 (first box height, ignoring its offset)"
    );
}

#[test]
fn test_nested_padding_accumulates() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut composition = run_test_composition(|| {
        Box(Modifier::empty().padding(10.0), BoxSpec::default(), || {
            Box(Modifier::empty().padding(5.0), BoxSpec::default(), || {
                Box(
                    Modifier::empty().size(Size {
                        width: 30.0,
                        height: 30.0,
                    }),
                    BoxSpec::default(),
                    || {},
                );
            });
        });
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

    let outer_rect = &layout.root().rect;
    assert_eq!(outer_rect.width, 60.0, "Outer width should be 60");
    assert_eq!(outer_rect.height, 60.0, "Outer height should be 60");

    let middle_rect = &layout.root().children[0].rect;
    assert_eq!(middle_rect.width, 40.0, "Middle width should be 40");
    assert_eq!(middle_rect.height, 40.0, "Middle height should be 40");
    assert_eq!(
        middle_rect.x, 10.0,
        "Middle box should be offset by outer padding (10)"
    );
    assert_eq!(
        middle_rect.y, 10.0,
        "Middle box should be offset by outer padding (10)"
    );

    let inner_rect = &layout.root().children[0].children[0].rect;
    assert_eq!(inner_rect.width, 30.0, "Inner width should be 30");
    assert_eq!(inner_rect.height, 30.0, "Inner height should be 30");
    assert_eq!(
        inner_rect.x, 15.0,
        "Inner box should be at outer padding (10) + middle padding (5)"
    );
    assert_eq!(
        inner_rect.y, 15.0,
        "Inner box should be at outer padding (10) + middle padding (5)"
    );
}
