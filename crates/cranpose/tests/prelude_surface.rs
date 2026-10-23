use cranpose::prelude::*;

#[test]
fn a_text_field_composes_from_the_prelude_alone() {
    run_test_composition(|| {
        let state = TextFieldState::new("hello\nworld");
        state.set_selection(TextRange::new(0, 5));
        BasicTextFieldWithOptions(
            state,
            Modifier::empty(),
            BasicTextFieldOptions {
                line_limits: TextFieldLineLimits::MultiLine {
                    min_lines: 1,
                    max_lines: 3,
                },
                ..BasicTextFieldOptions::default()
            },
        );

        state.edit(|buffer: &mut TextFieldBuffer| buffer.place_cursor_at_end());
        let value: TextFieldValue = state.value();
        assert_eq!(value.text, "hello\nworld");
        assert_eq!(value.selection, TextRange::cursor("hello\nworld".len()));
    });
}

#[test]
fn app_owned_list_data_composes_from_the_prelude_alone() {
    run_test_composition(|| {
        let items: SnapshotStateList<String> =
            mutableStateListOf(["one".to_string(), "two".to_string()]);
        items.push("three".to_string());
        assert_eq!(items.len(), 3);

        let empty: SnapshotStateList<u32> = mutableStateList();
        assert!(empty.is_empty(), "a fresh state list arrived with contents");

        let state = rememberLazyListState();
        LazyColumn(
            Modifier::empty(),
            state,
            LazyColumnSpec::default(),
            move |scope| {
                scope.items(items.len(), move |index| {
                    let _label = items.get(index);
                    Spacer(Modifier::empty().size_points(50.0, 20.0));
                });
            },
        );
    });
}

#[test]
fn app_owned_map_data_composes_from_the_prelude_alone() {
    run_test_composition(|| {
        let counts: SnapshotStateMap<String, u32> = mutableStateMapOf([("one".to_string(), 1u32)]);
        counts.insert("two".to_string(), 2);
        assert_eq!(counts.len(), 2);

        let empty: SnapshotStateMap<String, u32> = mutableStateMap();
        assert!(empty.is_empty(), "a fresh state map arrived with entries");
    });
}

#[test]
fn effects_and_key_scope_compose_from_the_prelude_alone() {
    let ran = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let effect_ran = std::rc::Rc::clone(&ran);
    run_test_composition(move || {
        let effect_ran = std::rc::Rc::clone(&effect_ran);
        key("scoped", || {
            LaunchedEffect((), move |_scope| {
                effect_ran.set(effect_ran.get() + 1);
            });
        });
        DisposableEffect((), |scope| scope.on_dispose(|| {}));
    });
    assert_eq!(
        ran.get(),
        1,
        "LaunchedEffect nested in key(..) must run once, using only cranpose::prelude::*"
    );
}

#[test]
fn keyed_remember_is_available_from_the_prelude() {
    run_test_composition(|| {
        let length = rememberKeyed("planet", |name| name.len());
        assert_eq!(length, 6);
    });
}

#[test]
fn blocking_work_requires_a_runtime_without_running_on_the_caller() {
    let result = std::rc::Rc::new(std::cell::Cell::new(false));
    let sink = std::rc::Rc::clone(&result);
    let launch = launchBlocking(
        || panic!("must not run inline"),
        move |_: Result<(), BlockingError>| sink.set(true),
    );
    assert!(matches!(launch, Err(BlockingError::NoRuntime)));
    assert!(!result.get());
}

#[test]
fn tiled_brushes_and_edge_blurs_draw_from_the_prelude_alone() {
    let stripes = Brush::vertical_gradient_tiled(
        vec![Color::BLACK, Color::WHITE],
        0.0,
        2.0,
        TileMode::Repeated,
    );
    let rings = Brush::radial_gradient_tiled(
        vec![Color::BLACK, Color::WHITE],
        Point::new(20.0, 20.0),
        8.0,
        TileMode::Mirror,
    );
    let blur = RenderEffect::blur_with_edge_treatment(4.0, TileMode::Decal);
    assert!(!blur.contains_runtime_shader());
    run_test_composition(move || {
        let (stripes, rings) = (stripes.clone(), rings.clone());
        Box(
            Modifier::empty()
                .size_points(40.0, 40.0)
                .draw_behind(move |scope| {
                    scope.draw_rect(stripes.clone());
                    scope.draw_rect(rings.clone());
                }),
            BoxSpec::default(),
            || {},
        );
    });
}

#[test]
fn runtime_shader_settings_are_named_from_the_prelude_alone() {
    let mut shader =
        RuntimeShader::new("@fragment fn main() -> @location(0) vec4<f32> { return vec4(1.0); }");
    let refused: Result<(), RuntimeShaderUniformError> = shader.try_set_float(usize::MAX, 1.0);
    assert!(refused.is_err());
    let substrates: &[SubstrateSpec] = shader.substrates();
    assert!(substrates.is_empty());
    let placeholder: Option<ShaderPlaceholder> = shader.placeholder();
    let shape: Option<PlaceholderShape> = placeholder.and_then(|placeholder| placeholder.shape);
    assert!(shape.is_none());
    let specialization: DrawSpecialization<'_> = shader.draw_specialization(0);
    assert!(specialization.overrides().is_empty());
    assert!(matches!(
        BlurredEdgeTreatment::UNBOUNDED.tile_mode(),
        TileMode::Decal
    ));
}

#[test]
fn drag_and_scroll_axes_are_named_from_the_prelude_alone() {
    run_test_composition(|| {
        let horizontal = rememberDraggableState(|_delta| {});
        let guarded = rememberDraggableState(|_delta| {});
        let scrolled = rememberScrollableState(|_delta| 0.0);
        Box(
            Modifier::empty()
                .size_points(40.0, 40.0)
                .draggable(Axis::Horizontal, horizontal)
                .draggable_guarded(Axis::Vertical, guarded, || true)
                .scrollable(Axis::Horizontal, scrolled),
            BoxSpec::default(),
            || {},
        );
    });
}
