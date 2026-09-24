#[path = "../../../crates/cranpose-render/wgpu/tests/support/device.rs"]
mod gpu_test_device;
#[path = "../src/test_screens/liquid_tab_reference.rs"]
#[expect(dead_code)]
mod reference;

mod accessibility_audit;
mod composition_local_dup_test;
mod counter_tab_roundtrip_regression;
mod demo_fonts_parse;
mod demo_tab_navigation_test;
mod font_variations;
mod liquid_page_support;
mod liquid_pass_inventory;
mod liquid_scroll_phase;
mod mineswapper_lazy_list_regression;
mod modifier_showcase_layout_tests;
mod modifier_showcase_rendering_tests;
mod modifier_showcase_tests;
mod recursive_layout_tab_switch_regression;
mod reference_content;
mod robot_rich_text_bounds_test;
mod scroll_repro_tests;
mod source_hygiene_aliases;
mod tab_switch_regression_support;
mod text_wrap_repro_tests;
mod variable_font_instancing;
mod web_release_bundle;
mod winamp_tab_integration_test;
