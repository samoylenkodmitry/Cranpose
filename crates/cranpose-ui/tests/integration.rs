mod api_surface;
mod baseline_alignment_integration;
mod basic_text_field_style_integration;
mod cjk_line_breaks;
mod debug_and_draw_scopes;
mod directional_layout;
mod draw_scope_text_integration;
mod flex_box;
mod flow_column;
mod geometry_and_modifiers;
mod graphics_layer_backdrop_integration;
mod grid;
mod inspector_and_insets;
mod lazy_content_padding;
mod lazy_retained_many_roots;
#[cfg(feature = "localization")]
mod localization;
mod parent_data_order;
mod round_scaling_list_integration;
mod round_scroll_indicator_integration;
mod semantics_hint;
mod shadow_api_integration;
mod static_density_integration;
mod subcompose_disposal;
#[cfg(feature = "svg")]
mod svg_painter_integration;
mod text_contract_measurer;
mod text_hyphenation_contract_integration;
mod text_measured_layout_integration;
mod text_options_contract_integration;
mod text_overflow_ellipsis_integration;
mod text_prepared_cache_annotations_integration;
#[cfg(feature = "southeast-asian-line-breaks")]
mod thai_line_breaks;
#[path = "support/thread_exit.rs"]
mod thread_exit;
mod thread_exit_teardown;
mod wear_list_and_painters;
mod widget_composition;
mod window_root_routing_scratch;
mod with_content_once_integration;
