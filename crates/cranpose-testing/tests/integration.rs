//! Every integration test of this crate, linked as one binary: one link
//! instead of one per file, while nextest still runs each test in its own
//! process.

mod button_clickable_test;
mod composition_switching_test;
mod conditional_rendering_test;
mod integration_modifier_tests;
mod intrinsics_test;
mod modifier_offset_test;
mod modifier_reuse_test;
mod modifier_size_position_test;
mod placed_semantics_test;
mod pointer_hover_gradient_test;
mod pointer_icon_end_to_end_test;
mod pointer_input_end_to_end_test;
mod pointer_input_integration_test;
mod remember_keyed_test;
mod render_invalidation_test;
mod robot_input;
