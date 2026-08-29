use cranpose_ui::*;

#[test]
fn test_padding_plus_size_constraint() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut composition = run_test_composition(|| {
        Box(
            Modifier::empty()
                .size(Size {
                    width: 100.0,
                    height: 100.0,
                })
                .padding(15.0),
            BoxSpec::default(),
            || {
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
        outer_rect.width, 100.0,
        "Outer width should be constrained to 100"
    );
    assert_eq!(
        outer_rect.height, 100.0,
        "Outer height should be constrained to 100"
    );
    assert_eq!(outer_rect.x, 0.0);
    assert_eq!(outer_rect.y, 0.0);

    let inner_rect = &layout.root().children[0].rect;
    assert_eq!(inner_rect.width, 50.0, "Inner width should be 50");
    assert_eq!(inner_rect.height, 50.0, "Inner height should be 50");
    assert_eq!(inner_rect.x, 15.0, "Inner x should be offset by padding");
    assert_eq!(inner_rect.y, 15.0, "Inner y should be offset by padding");
}

#[test]
fn test_column_with_padding_and_sized_children() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut composition = run_test_composition(|| {
        Column(
            Modifier::empty().padding(10.0),
            ColumnSpec::default(),
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
                        width: 60.0,
                        height: 40.0,
                    }),
                    BoxSpec::default(),
                    || {},
                );
                Box(
                    Modifier::empty().size(Size {
                        width: 40.0,
                        height: 20.0,
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

    let column_rect = &layout.root().rect;
    assert_eq!(
        column_rect.width, 80.0,
        "Column width = max child (60) + padding (20)"
    );
    assert_eq!(
        column_rect.height, 110.0,
        "Column height = sum children (90) + padding (20)"
    );

    assert_eq!(layout.root().children.len(), 3, "Should have 3 children");

    let child1 = &layout.root().children[0].rect;
    assert_eq!(child1.width, 50.0);
    assert_eq!(child1.height, 30.0);
    assert_eq!(child1.x, 10.0, "First child x = padding left");
    assert_eq!(child1.y, 10.0, "First child y = padding top");

    let child2 = &layout.root().children[1].rect;
    assert_eq!(child2.width, 60.0);
    assert_eq!(child2.height, 40.0);
    assert_eq!(child2.x, 10.0, "Second child x = padding left");
    assert_eq!(
        child2.y, 40.0,
        "Second child y = padding top (10) + first child height (30)"
    );

    let child3 = &layout.root().children[2].rect;
    assert_eq!(child3.width, 40.0);
    assert_eq!(child3.height, 20.0);
    assert_eq!(child3.x, 10.0, "Third child x = padding left");
    assert_eq!(
        child3.y, 80.0,
        "Third child y = padding top (10) + first (30) + second (40)"
    );
}

#[test]
fn test_row_with_offset_children() {
    let _app_context = cranpose_ui::AppContext::new();
    let _app_context_scope = _app_context.enter_scope();
    _app_context.enter(cranpose_ui::reset_render_state_for_tests);
    let mut composition = run_test_composition(|| {
        Row(Modifier::empty(), RowSpec::default(), || {
            Box(
                Modifier::empty().size(Size {
                    width: 40.0,
                    height: 30.0,
                }),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .size(Size {
                        width: 50.0,
                        height: 35.0,
                    })
                    .offset(5.0, 10.0),
                BoxSpec::default(),
                || {},
            );
            Box(
                Modifier::empty()
                    .size(Size {
                        width: 30.0,
                        height: 25.0,
                    })
                    .offset(0.0, -5.0),
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

    let row_rect = &layout.root().rect;
    assert_eq!(row_rect.width, 120.0, "Row width = sum of children");
    assert_eq!(row_rect.height, 35.0, "Row height = max child height");

    assert_eq!(layout.root().children.len(), 3, "Should have 3 children");

    let child1 = &layout.root().children[0].rect;
    assert_eq!(child1.width, 40.0);
    assert_eq!(child1.height, 30.0);
    assert_eq!(child1.x, 0.0, "First child at start");
    assert_eq!(child1.y, 2.5, "First child vertically centered: (35-30)/2");

    let child2 = &layout.root().children[1].rect;
    assert_eq!(child2.width, 50.0);
    assert_eq!(child2.height, 35.0);
    assert_eq!(
        child2.x, 45.0,
        "Second child x = first width (40) + offset x (5)"
    );
    assert_eq!(child2.y, 10.0, "Second child y = offset y (10)");

    let child3 = &layout.root().children[2].rect;
    assert_eq!(child3.width, 30.0);
    assert_eq!(child3.height, 25.0);
    assert_eq!(
        child3.x, 90.0,
        "Third child x = first (40) + second (50) + offset (0)"
    );
    assert_eq!(child3.y, 0.0, "Third child y = center (5) + offset (-5)");
}
