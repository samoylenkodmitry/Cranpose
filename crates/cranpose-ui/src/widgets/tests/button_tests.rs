use cranpose_core::{Composition, MemoryApplier};

use super::*;

#[test]
fn default_button_spec_has_no_interaction_source() {
    let spec = ButtonSpec::default();

    assert!(spec.interaction_source.is_none());
}

#[test]
fn button_spec_builder_preserves_interaction_source() {
    let composition = Composition::new(MemoryApplier::new());
    let source = MutableInteractionSource::with_runtime(composition.runtime_handle());
    let spec = ButtonSpec::new().interaction_source(source);

    assert_eq!(spec.interaction_source, Some(source));
}
