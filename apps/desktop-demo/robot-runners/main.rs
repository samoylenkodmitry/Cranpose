use std::process::{ExitCode, Termination};

mod external_x11_frame_telemetry;
mod glass_backdrop_scroll_helpers;
mod hacker_news_robot_support;
mod liquid_cheatsheets;
mod liquid_page;
#[path = "../src/test_screens/liquid_tab_reference.rs"]
mod liquid_tab_reference;
mod markdown_fixture_client;
mod markdown_scroll_drag;
mod output_paths;
mod perf_contract;
mod perf_presentation_probe;
mod perf_robot_stats;
mod presented_window_geometry_contract;
mod regression_robot_support;
mod robot_exit;
mod robot_handle_probe;
mod robot_launch;
mod robot_liquid_stage;
mod robot_pixels;
mod robot_shot;
mod robot_tab_fixture;
mod scroll_stability_external_helpers;
mod text_fixture_style;
mod text_input_robot_helpers;
mod text_showcase_external_helpers;
mod visual_contract_metrics;

macro_rules! runners {
    ($($runner:ident),* $(,)?) => {
        $(mod $runner;)*

        const RUNNERS: &[(&str, fn() -> ExitCode)] =
            &[$((stringify!($runner), || $runner::main().report())),*];
    };
}

runners! {
    robot_adaptive_frost,
    robot_advance_frame_bug,
    robot_android_resume,
    robot_animation_showcase,
    robot_async_pause,
    robot_async_progress_after_animations,
    robot_async_tab_bug,
    robot_click_drag,
    robot_color_fidelity,
    robot_composition_local_disappear,
    robot_conditional_infinite_transition_busy,
    robot_content_type_reuse,
    robot_controls_grid,
    robot_copy_paste,
    robot_counter_button_release_external_visual,
    robot_counter_conditional_text_top,
    robot_demo,
    robot_developer_inspector,
    robot_double_click,
    robot_drag_selection,
    robot_draw_only_transition_work,
    robot_feed_touched_shadow,
    robot_fling,
    robot_fling_edge_cases,
    robot_fling_interrupt,
    robot_fling_precise,
    robot_focus_requester,
    robot_font_scale_first_frame,
    robot_glass_backdrop_scroll_headless,
    robot_glass_backdrop_scroll_stability,
    robot_glass_feed_wheel,
    robot_glass_physical_depth,
    robot_glass_tiles,
    robot_gradient_tab_switch,
    robot_hacker_news_back_drag_scroll,
    robot_hacker_news_scroll,
    robot_hacker_news_scroll_exact_external_contract,
    robot_idle_fps_after_tab_walk,
    robot_idle_transition_work,
    robot_image_chessboard,
    robot_increment_bug,
    robot_interactive,
    robot_interactive_anim,
    robot_layout_validation,
    robot_lazy_complex_scroll,
    robot_lazy_duplication_bug,
    robot_lazy_expansion_reflow,
    robot_lazy_extreme_nav,
    robot_lazy_fixes,
    robot_lazy_infinite_scroll,
    robot_lazy_items_provider,
    robot_lazy_items_rc,
    robot_lazy_lifecycle,
    robot_lazy_list,
    robot_lazy_list_after_modifiers,
    robot_lazy_list_end_alignment,
    robot_lazy_list_end_start_dup,
    robot_lazy_list_order_bug,
    robot_lazy_list_remove_dup,
    robot_lazy_list_state_reactivity,
    robot_lazy_max_demo,
    robot_lazy_max_jump,
    robot_lazy_perf,
    robot_lazy_perf_validation,
    robot_lazy_recursive_composition,
    robot_lazy_scroll_edge_cases,
    robot_lazy_stats,
    robot_lazy_tab_test,
    robot_lazy_varheight_lifecycle,
    robot_lazy_variable_height,
    robot_lazy_wheel_backtrack,
    robot_lazylist_redraw_bug,
    robot_leetcodedaily_code_scroll_pixel_drift,
    robot_leetcodedaily_full_layout_scroll_stability,
    robot_light_menu,
    robot_liquid_backdrop_feedback,
    robot_liquid_bar_alignment,
    robot_liquid_bottom_bar_form_cheatsheet,
    robot_liquid_dropdown_accordion,
    robot_liquid_menu_expand_cheatsheet,
    robot_liquid_menu_open_cheatsheet,
    robot_liquid_nav_bar_backdrop,
    robot_liquid_navbar_touch_budget,
    robot_liquid_on_white_click_cheatsheet,
    robot_liquid_on_white_click_hold_cheatsheet,
    robot_liquid_on_white_touched_up_cheatsheet,
    robot_liquid_round_glass_edge_refraction,
    robot_liquid_scroll_exact_external_contract,
    robot_liquid_segmented_cheatsheet,
    robot_liquid_segmented_glide_budget,
    robot_liquid_tab_bar_pill_containment,
    robot_liquid_tab_content_anchor,
    robot_liquid_tab_flight_dark_scheme_ink_recolor,
    robot_liquid_tab_ink_spectrum,
    robot_liquid_tab_press_preview,
    robot_liquid_tab_settled_blur,
    robot_liquid_tab_swipe_cheatsheet,
    robot_liquid_text_selection_cheatsheet,
    robot_liquid_toggle_press_cheatsheet,
    robot_liquid_touched_shadow,
    robot_liquid_visual,
    robot_markdown_default_visual_contract,
    robot_markdown_end_drag_up,
    robot_markdown_full_demo_code_block_visual_contract,
    robot_markdown_scroll_exact_external_contract,
    robot_markdown_scrollbar,
    robot_measure_shaders,
    robot_memory_leak,
    robot_menu_slide,
    robot_modifier_render,
    robot_multiline_click,
    robot_multiline_nav,
    robot_nested_glass_animated,
    robot_nested_glass_cache,
    robot_no_fling_recording2,
    robot_novsync_free_runs,
    robot_offscreen_draw_transition,
    robot_offset_test,
    robot_overscroll_bounce,
    robot_perf_harness,
    robot_positioned_boxes_after_lazy_list,
    robot_presented_window_geometry,
    robot_presented_window_hidpi_geometry,
    robot_presented_window_redraw,
    robot_pressed_state,
    robot_progress_bar,
    robot_reactive_handle_copy,
    robot_reactive_state,
    robot_recomposition_lab,
    robot_recursive_layout,
    robot_recursive_layout_depth_six,
    robot_regression_fused_viewport_contract,
    robot_regression_hn_markdown_visual_contract,
    robot_regression_layout_jitter_contract,
    robot_regression_shader_visual_contract,
    robot_regression_tabbar_contract,
    robot_render_crispness_contract,
    robot_render_translation_contract,
    robot_renderer_micro_contract,
    robot_renderer_pixel_precision,
    robot_request_exit,
    robot_rotated_glass_shape,
    robot_scroll_bug,
    robot_scroll_decoration_invariance,
    robot_scroll_jump,
    robot_scroll_persistence,
    robot_scroll_to_item_redraw,
    robot_scroll_visual,
    robot_selection_vertical_grab,
    robot_services_registration,
    robot_shader_backdrop_drag,
    robot_shader_external_x11_drag,
    robot_shader_full_demo_external_perf,
    robot_shader_rect,
    robot_shader_rect_external_animation,
    robot_shadow_fields,
    robot_subcompose_invalidation,
    robot_subcompose_lazy,
    robot_subcompose_loop_disposal,
    robot_tab_navigation,
    robot_tab_roundtrip_content,
    robot_tab_screenshot_dump,
    robot_tab_scroll,
    robot_tab_selection,
    robot_tab_walk_text_visual_contract,
    robot_tabbar_drag,
    robot_tabs_scroll,
    robot_test_recorder,
    robot_text_handle_cycle_stability,
    robot_text_handle_survives_tab_switch,
    robot_text_indent_scroll_stability,
    robot_text_input,
    robot_text_input_value_disappear,
    robot_text_loupe,
    robot_text_scroll_exact_external_contract,
    robot_text_showcase_gradient,
    robot_text_strikeout_presented,
    robot_ui_breakage,
    robot_underline_screenshot,
    robot_viewport_uniform_growth,
    robot_wear_watch_fps,
    robot_winamp_native_window_geometry,
    robot_xwayland_display_backend,
}

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("--list") => {
            for (name, _) in RUNNERS {
                println!("{name}");
            }
            ExitCode::SUCCESS
        }
        Some(requested) => match RUNNERS.iter().find(|(name, _)| *name == requested) {
            Some((_, run)) => run(),
            None => {
                eprintln!("no robot runner named `{requested}`; `robot --list` prints them all");
                ExitCode::FAILURE
            }
        },
        None => {
            eprintln!("usage: robot <runner>  |  robot --list");
            ExitCode::FAILURE
        }
    }
}
