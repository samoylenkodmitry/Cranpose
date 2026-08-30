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
                    Spacer(Size::new(50.0, 20.0));
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
fn blocking_work_starts_from_the_prelude_alone() {
    let result = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let sink = std::rc::Rc::clone(&result);
    launchBlocking(|| 2 + 2, move |sum| sink.set(sum));
    assert_eq!(
        result.get(),
        4,
        "with no runtime the work runs inline, so the result is already here"
    );
}
