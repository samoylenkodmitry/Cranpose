#[test]
fn value_slot_handle_cannot_escape_composable_scope() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/value_slot_handle_escape_body.rs");
    cases.compile_fail("tests/ui/value_slot_handle_return.rs");
    cases.compile_fail("tests/ui/value_slot_handle_state.rs");
}
