use super::*;

thread_local! {
    static TRACED_SCOPE_ID: Cell<Option<usize>> = const { Cell::new(None) };
}

#[composable]
fn traced_reader(state: MutableState<i32>) {
    debug_label_current_scope("traced_reader_body");
    with_current_composer(|composer| {
        let scope = composer.current_recompose_scope().expect("scope available");
        TRACED_SCOPE_ID.with(|slot| slot.set(Some(scope.id())));
    });
    let _ = state.value();
}

fn trace_invalidated_scope(tracking: bool) -> (Option<&'static str>, Vec<String>) {
    DEBUG_SCOPE_TRACKING_OVERRIDE.with(|enabled| enabled.set(Some(tracking)));
    let mut composition = test_composition();
    let runtime = composition.runtime_handle();
    let state = MutableState::with_runtime(0, runtime);
    let root_key = location_key(file!(), line!(), column!());

    composition
        .render(root_key, || traced_reader(state))
        .expect("initial composition");
    state.set_value(1);
    let _ = composition
        .process_invalid_scopes()
        .expect("recomposition after invalidation");

    let scope_id = TRACED_SCOPE_ID.with(Cell::get).expect("traced scope id");
    let trace = (
        debug_scope_label(scope_id),
        debug_scope_invalidation_sources(scope_id),
    );
    DEBUG_SCOPE_TRACKING_OVERRIDE.with(|enabled| enabled.set(None));
    trace
}

#[test]
fn scope_tracking_records_label_and_invalidation_source() {
    let (label, sources) = trace_invalidated_scope(true);

    assert_eq!(label, Some("traced_reader_body"));
    assert_eq!(
        sources.len(),
        1,
        "one state invalidated the scope: {sources:?}"
    );
    assert!(
        sources[0].starts_with("slot=") && sources[0].ends_with(" i32"),
        "the source names the state slot and value type: {sources:?}",
    );
}

#[test]
fn disabled_scope_tracking_records_nothing() {
    let (label, sources) = trace_invalidated_scope(false);

    assert_eq!(label, None);
    assert!(sources.is_empty(), "no source recorded: {sources:?}");
}
