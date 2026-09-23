use super::*;

#[deny(non_snake_case)]
mod camel_case {
    use std::cell::Cell;

    use super::*;

    thread_local! {
        pub(super) static RENDERED: Cell<i32> = const { Cell::new(0) };
    }

    #[composable]
    pub(super) fn CamelCaseProbe(marker: i32) {
        RENDERED.with(|rendered| rendered.set(marker));
    }
}

#[test]
fn a_camel_case_composable_compiles_where_non_snake_case_is_denied() {
    let mut composition = test_composition();

    composition
        .render(3, || camel_case::CamelCaseProbe(7))
        .expect("compose the camel-case probe");

    assert_eq!(camel_case::RENDERED.with(Cell::get), 7);
}
