use cranpose_core::{Composition, MemoryApplier, location_key};

use super::*;

#[test]
fn clickable_text_composes_without_panic() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut comp = Composition::new(MemoryApplier::new());
    comp.render(location_key(file!(), line!(), column!()), || {
        ClickableText(
            AnnotatedString::from("Hello"),
            Modifier::empty(),
            TextStyle::default(),
            |_offset| {},
        );
    })
    .expect("composition succeeds");
}
