use super::{
    collect_slices_from_modifier, inspector_metadata, modifier_element, Alignment, BlendMode,
    Color, CompositingStrategy, CutDirection, DimensionConstraint, DpOffset, DrawCommand,
    DynModifierElement, EdgeInsets, GlassMaterial, GradientCutMaskSpec, GradientFadeMaskSpec,
    GraphicsLayer, HorizontalAlignment, LayerShape, Modifier, ModifierChainHandle, Point,
    RenderEffect, RoundedCornerShape, RuntimeShader, SemanticsConfiguration, Shadow, Size,
    TransformOrigin, VerticalAlignment,
};
use cranpose_foundation::{
    DelegatableNode, ModifierNode, ModifierNodeElement, NodeCapabilities, NodeState,
};
use cranpose_ui_graphics::{
    BlurredEdgeTreatment, Brush, ColorFilter, Dp, DrawPrimitive, ShadowPrimitive, TileMode,
};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

#[test]
fn modifier_debug_env_flag_is_not_process_cached() {
    let source = include_str!("../mod.rs");
    let once_lock = ["Once", "Lock"].concat();
    let env_debug = ["static ", "ENV_DEBUG"].concat();

    assert!(
        !source.contains(&once_lock) && !source.contains(&env_debug),
        "modifier debug env flag must not be latched in a process-global cache"
    );
}

struct DensityGuard(f32);

impl DensityGuard {
    fn set(density: f32) -> Self {
        let previous_density = crate::current_density();
        crate::set_density(density);
        Self(previous_density)
    }
}

impl Drop for DensityGuard {
    fn drop(&mut self) {
        crate::set_density(self.0);
    }
}

#[test]
fn padding_nodes_resolve_padding_values() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty()
        .padding(4.0)
        .then(Modifier::empty().padding_horizontal(2.0))
        .then(Modifier::empty().padding_each(1.0, 3.0, 5.0, 7.0));
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);
    let padding = handle.resolved_modifiers().padding();
    assert_eq!(
        padding,
        EdgeInsets {
            left: 7.0,
            top: 7.0,
            right: 11.0,
            bottom: 11.0,
        }
    );
}

#[test]
fn fill_max_size_sets_fraction_constraints() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().fill_max_size_fraction(0.75);
    let props = modifier.resolved_modifiers().layout_properties();
    assert_eq!(props.width(), DimensionConstraint::Fraction(0.75));
    assert_eq!(props.height(), DimensionConstraint::Fraction(0.75));
}

#[test]
fn weight_tracks_fill_flag() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().weight_with_fill(2.0, false);
    let props = modifier.resolved_modifiers().layout_properties();
    let weight = props.weight().expect("weight to be recorded");
    assert_eq!(weight.weight, 2.0);
    assert!(!weight.fill);
}

#[test]
fn from_element_exposes_custom_modifier_nodes() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::from_element(TestFingerprintElement {
        id: 7,
        capabilities: NodeCapabilities::DRAW,
        always_update: false,
    });
    let mut handle = ModifierChainHandle::new();

    handle.update(&modifier);

    assert!(handle.has_draw_nodes());
    assert!(handle
        .chain()
        .has_nodes_for_invalidation(cranpose_foundation::InvalidationKind::Draw));
}

#[test]
fn offset_accumulates_across_chain() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty()
        .offset(4.0, 6.0)
        .then(Modifier::empty().absolute_offset(-1.5, 2.5))
        .then(Modifier::empty().offset(0.5, -3.0));
    let total = modifier.resolved_modifiers().offset();
    assert_eq!(total, Point { x: 3.0, y: 5.5 });
}

#[test]
fn lazy_scroll_modifier_keeps_motion_context_inactive_at_rest() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut list_state = None;
    let _composition = crate::run_test_composition(|| {
        list_state = Some(cranpose_foundation::lazy::remember_lazy_list_state());
    });
    let modifier = Modifier::empty().lazy_vertical_scroll(
        list_state.expect("lazy list state should be created"),
        false,
    );
    let slices = collect_slices_from_modifier(&modifier);

    assert!(!slices.motion_context_animated());
}

#[test]
fn regular_scroll_modifier_keeps_translated_content_context_active_at_rest() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut state = None;
    let _composition = crate::run_test_composition(|| {
        state = Some(crate::ScrollState::new(12.0));
    });
    let modifier =
        Modifier::empty().vertical_scroll(state.expect("scroll state should be created"), false);
    let slices = collect_slices_from_modifier(&modifier);

    assert!(slices.translated_content_context());
}

#[test]
fn regular_scroll_modifier_does_not_draw_implicit_scrollbar() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut state = None;
    let _composition = crate::run_test_composition(|| {
        state = Some(crate::ScrollState::new(0.0));
    });
    let state = state.expect("scroll state should be created in a runtime");
    state.set_max_value(600.0);
    state.scroll_to(300.0);
    let modifier = Modifier::empty().vertical_scroll(state, false);
    let slices = collect_slices_from_modifier(&modifier);
    assert!(
        slices.draw_commands().is_empty(),
        "plain scroll modifiers should not draw visual chrome; scrollbars belong to explicit scrollbar components"
    );
}

#[test]
fn lazy_scroll_modifier_keeps_translated_content_context_active_at_rest() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut list_state = None;
    let _composition = crate::run_test_composition(|| {
        list_state = Some(cranpose_foundation::lazy::remember_lazy_list_state());
    });
    let modifier = Modifier::empty().lazy_vertical_scroll(
        list_state.expect("lazy list state should be created"),
        false,
    );
    let slices = collect_slices_from_modifier(&modifier);

    assert!(slices.translated_content_context());
}

#[test]
fn modifier_slices_debug_stats_report_counts_and_capacities() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty()
        .background(Color::rgba(0.2, 0.4, 0.6, 1.0))
        .clickable(|_| {});
    let slices = collect_slices_from_modifier(&modifier);
    let stats = slices.debug_stats();

    assert!(
        stats.draw_command_count >= 1,
        "background should produce at least one draw command: {stats:?}"
    );
    assert!(stats.draw_command_capacity >= stats.draw_command_count);
    assert_eq!(stats.pointer_input_count, 1);
    assert!(stats.pointer_input_capacity >= stats.pointer_input_count);
    assert!(stats.heap_bytes > 0);
}

#[test]
fn then_short_circuits_empty_modifiers() {
    let _app_context = crate::render_state::app_context_test_scope();
    let padding = Modifier::empty().padding(4.0);
    assert_eq!(Modifier::empty().then(padding.clone()), padding);

    let background = Modifier::empty().background(Color::rgba(0.2, 0.4, 0.6, 1.0));
    assert_eq!(background.then(Modifier::empty()), background);
}

#[test]
fn structural_eq_ignores_always_update_elements() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier_a = Modifier::empty().draw_behind({
        let width = 24.0;
        move |_scope| {
            let _ = width;
        }
    });
    let modifier_b = Modifier::empty().draw_behind({
        let width = 120.0;
        move |_scope| {
            let _ = width;
        }
    });

    assert!(modifier_a.structural_eq(&modifier_b));
    assert_ne!(modifier_a, modifier_b);
}

#[test]
fn incremental_single_fingerprints_match_full_pass() {
    let _app_context = crate::render_state::app_context_test_scope();
    let elements = vec![
        test_fingerprint_element(1, NodeCapabilities::LAYOUT, false),
        test_fingerprint_element(2, NodeCapabilities::DRAW, false),
        test_fingerprint_element(3, NodeCapabilities::DRAW, true),
        test_fingerprint_element(4, NodeCapabilities::SEMANTICS, false),
    ];

    let seeded = super::append_fingerprints(super::single_fingerprint_seed(), &elements[..2]);
    let split = super::append_fingerprints(seeded, &elements[2..]);

    assert_eq!(super::single_fingerprints(&elements), split);
}

#[test]
fn modifiers_built_incrementally_match_from_parts() {
    let _app_context = crate::render_state::app_context_test_scope();
    let elements = vec![
        test_fingerprint_element(10, NodeCapabilities::LAYOUT, false),
        test_fingerprint_element(11, NodeCapabilities::DRAW, false),
        test_fingerprint_element(12, NodeCapabilities::SEMANTICS, false),
    ];

    let flat = Modifier::from_parts(elements.clone());
    let incremental = Modifier::from_parts(vec![elements[0].clone()])
        .then(Modifier::from_parts(vec![elements[1].clone()]))
        .then(Modifier::from_parts(vec![elements[2].clone()]));

    assert_eq!(incremental, flat);
    assert!(incremental.structural_eq(&flat));
}

#[test]
fn required_size_sets_explicit_constraints() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().required_size(Size {
        width: 32.0,
        height: 18.0,
    });
    let props = modifier.resolved_modifiers().layout_properties();
    assert_eq!(props.width(), DimensionConstraint::Points(32.0));
    assert_eq!(props.height(), DimensionConstraint::Points(18.0));
    assert_eq!(props.min_width(), Some(32.0));
    assert_eq!(props.max_width(), Some(32.0));
    assert_eq!(props.min_height(), Some(18.0));
    assert_eq!(props.max_height(), Some(18.0));
}

#[test]
fn alignment_modifiers_record_values() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty()
        .align(Alignment::BOTTOM_END)
        .alignInColumn(HorizontalAlignment::CenterHorizontally)
        .alignInRow(VerticalAlignment::Top);
    let props = modifier.resolved_modifiers().layout_properties();
    assert_eq!(props.box_alignment(), Some(Alignment::BOTTOM_END));
    assert_eq!(
        props.column_alignment(),
        Some(HorizontalAlignment::CenterHorizontally)
    );
    assert_eq!(props.row_alignment(), Some(VerticalAlignment::Top));
}

#[test]
fn graphics_layer_modifier_creates_node() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier::ModifierChainHandle;
    use crate::modifier_nodes::GraphicsLayerNode;

    let layer = GraphicsLayer {
        alpha: 0.5,
        ..Default::default()
    };
    let modifier = Modifier::empty().graphics_layer(move || layer.clone());

    // Graphics layer is now tracked in the modifier node chain, not ResolvedModifiers
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    // Verify the node exists in the chain by checking for DRAW capability
    let chain = handle.chain();
    let mut has_graphics_layer = false;
    chain.for_each_node_with_capability(
        cranpose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if node.as_any().downcast_ref::<GraphicsLayerNode>().is_some() {
                has_graphics_layer = true;
            }
        },
    );
    assert!(has_graphics_layer, "Expected GraphicsLayerNode in chain");
}

#[test]
fn backdrop_effect_modifier_creates_graphics_layer_with_backdrop_effect() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier_nodes::GraphicsLayerNode;

    let modifier = Modifier::empty().backdrop_effect(RenderEffect::blur(8.0));
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    let chain = handle.chain();
    let mut found = false;
    chain.for_each_node_with_capability(
        cranpose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if let Some(layer_node) = node.as_any().downcast_ref::<GraphicsLayerNode>() {
                let layer = layer_node.layer();
                found = layer.backdrop_effect.is_some();
            }
        },
    );

    assert!(
        found,
        "Expected backdrop_effect to be present in GraphicsLayer"
    );
}

#[test]
fn backdrop_blur_clips_backdrop_to_bounds() {
    let _app_context = crate::render_state::app_context_test_scope();
    let _density = DensityGuard::set(2.0);
    let modifier = Modifier::empty().backdrop_blur(Dp(12.0));
    let slices = collect_slices_from_modifier(&modifier);

    let layer = slices
        .graphics_layer()
        .expect("backdrop_blur should install a graphics layer");
    assert!(layer.clip);
    assert_eq!(layer.shape, LayerShape::Rectangle);
    match layer.backdrop_effect {
        Some(RenderEffect::Blur {
            radius_x, radius_y, ..
        }) => {
            assert_eq!(radius_x, 24.0);
            assert_eq!(radius_y, 24.0);
        }
        other => panic!("expected blur backdrop effect, got {other:?}"),
    }
}

#[test]
fn glass_material_applies_blur_tint_and_shape() {
    let _app_context = crate::render_state::app_context_test_scope();
    let _density = DensityGuard::set(1.5);
    let tint = Color::from_rgba_u8(245, 253, 255, 150);
    let shape = RoundedCornerShape::uniform(18.0);
    let modifier = Modifier::empty().glass_material(GlassMaterial::new(Dp(18.0), tint, shape));
    let slices = collect_slices_from_modifier(&modifier);

    assert_eq!(slices.corner_shape(), Some(shape));

    let layer = slices
        .graphics_layer()
        .expect("glass_material should install a graphics layer");
    assert!(layer.clip);
    assert_eq!(layer.shape, LayerShape::Rounded(shape));
    match layer.backdrop_effect {
        Some(RenderEffect::Blur {
            radius_x, radius_y, ..
        }) => {
            assert_eq!(radius_x, 27.0);
            assert_eq!(radius_y, 27.0);
        }
        other => panic!("expected glass_material blur backdrop, got {other:?}"),
    }

    let commands = slices.draw_commands();
    assert_eq!(commands.len(), 1);
    let DrawCommand::Behind(draw) = &commands[0] else {
        panic!("glass_material tint should render behind content");
    };
    let primitives = draw(Size {
        width: 100.0,
        height: 64.0,
    });
    let [DrawPrimitive::RoundRect { brush, .. }] = primitives.as_slice() else {
        panic!("glass_material tint should respect the rounded shape");
    };
    assert_eq!(*brush, Brush::Solid(tint));
}

#[test]
fn graphics_layer_reads_latest_value_without_recomposition() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier_nodes::GraphicsLayerNode;

    let alpha = Rc::new(Cell::new(0.25f32));
    let modifier = Modifier::empty().graphics_layer({
        let alpha = alpha.clone();
        move || GraphicsLayer {
            alpha: alpha.get(),
            ..Default::default()
        }
    });

    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    let chain = handle.chain();
    let read_alpha = |expected: f32| {
        let mut observed = None;
        chain.for_each_node_with_capability(
            cranpose_foundation::NodeCapabilities::DRAW,
            |_ref, node| {
                if let Some(layer_node) = node.as_any().downcast_ref::<GraphicsLayerNode>() {
                    observed = Some(layer_node.layer().alpha);
                }
            },
        );
        let value = observed.expect("graphics layer node");
        assert!((value - expected).abs() < 1e-6);
    };

    read_alpha(0.25);
    alpha.set(0.85);
    read_alpha(0.85);
}

#[test]
fn lazy_graphics_layer_state_writes_schedule_attached_node_draw_repass() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier_nodes::GraphicsLayerNode;

    let runtime =
        cranpose_core::runtime::Runtime::new(Arc::new(cranpose_core::runtime::DefaultScheduler));
    let x_state = cranpose_core::MutableState::with_runtime(10.0f32, runtime.handle());
    let modifier = Modifier::empty().graphics_layer({
        move || GraphicsLayer {
            translation_x: x_state.get(),
            ..Default::default()
        }
    });

    let mut handle = ModifierChainHandle::new();
    handle.set_node_id(Some(41));
    let _ = handle.update(&modifier);

    let chain = handle.chain();
    chain.for_each_node_with_capability(
        cranpose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if let Some(layer_node) = node.as_any().downcast_ref::<GraphicsLayerNode>() {
                assert!((layer_node.layer().translation_x - 10.0).abs() < 1e-6);
            }
        },
    );

    let _ = crate::take_render_invalidation();
    let _ = crate::take_draw_repass_nodes();
    x_state.set(42.0);
    runtime.handle().drain_ui();

    assert!(crate::take_render_invalidation());
    assert_eq!(crate::take_draw_repass_nodes(), vec![41]);
}

#[test]
fn lazy_graphics_layer_resolver_update_self_invalidates_without_auto_draw_invalidation() {
    let _app_context = crate::render_state::app_context_test_scope();

    let first = Modifier::empty().graphics_layer(|| GraphicsLayer {
        alpha: 0.25,
        ..Default::default()
    });
    let second = Modifier::empty().graphics_layer(|| GraphicsLayer {
        alpha: 0.75,
        ..Default::default()
    });

    let mut handle = ModifierChainHandle::new();
    handle.set_node_id(Some(42));
    let _ = handle.update(&first);
    let _ = handle.take_invalidations();
    let _ = crate::take_render_invalidation();
    let _ = crate::take_draw_repass_nodes();

    let _ = handle.update(&second);

    assert!(
        handle.take_invalidations().is_empty(),
        "lazy graphics layer resolver replacement schedules an exact draw repass itself"
    );
    assert!(crate::take_render_invalidation());
    assert_eq!(crate::take_draw_repass_nodes(), vec![42]);
}

#[test]
fn shader_background_wraps_runtime_shader_as_backdrop_effect() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier_nodes::GraphicsLayerNode;

    let modifier = Modifier::empty().shader_background(RuntimeShader::new(
        "@group(0) @binding(0) var input_texture: texture_2d<f32>;\n\
         @group(0) @binding(1) var input_sampler: sampler;\n\
         @group(1) @binding(0) var<uniform> u: array<vec4<f32>, 64>;\n\
         @vertex fn fullscreen_vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {\n\
             let x = f32(i32(i & 1u) * 2 - 1);\n\
             let y = f32(i32(i >> 1u) * 2 - 1);\n\
             return vec4<f32>(x, y, 0.0, 1.0);\n\
         }\n\
         @fragment fn effect_fs() -> @location(0) vec4<f32> { return vec4<f32>(0.0); }",
    ));
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    let chain = handle.chain();
    let mut found_shader = false;
    chain.for_each_node_with_capability(
        cranpose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if let Some(layer_node) = node.as_any().downcast_ref::<GraphicsLayerNode>() {
                found_shader = matches!(
                    layer_node.layer().backdrop_effect,
                    Some(RenderEffect::Shader { .. })
                );
            }
        },
    );

    assert!(
        found_shader,
        "Expected shader_background to configure backdrop shader"
    );
}

#[test]
fn color_filter_modifier_sets_graphics_layer_filter() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier_nodes::GraphicsLayerNode;

    let filter = ColorFilter::tint(Color::from_rgba_u8(128, 200, 255, 128));
    let modifier = Modifier::empty().color_filter(filter);
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    let chain = handle.chain();
    let mut observed = None;
    chain.for_each_node_with_capability(
        cranpose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if let Some(layer_node) = node.as_any().downcast_ref::<GraphicsLayerNode>() {
                observed = layer_node.layer().color_filter;
            }
        },
    );

    assert_eq!(observed, Some(filter));
}

#[test]
fn tint_modifier_is_color_filter_tint_alias() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier_nodes::GraphicsLayerNode;

    let tint = Color::from_rgba_u8(10, 20, 30, 200);
    let modifier = Modifier::empty().tint(tint);
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    let chain = handle.chain();
    let mut observed = None;
    chain.for_each_node_with_capability(
        cranpose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if let Some(layer_node) = node.as_any().downcast_ref::<GraphicsLayerNode>() {
                observed = layer_node.layer().color_filter;
            }
        },
    );

    assert_eq!(observed, Some(ColorFilter::tint(tint)));
}

#[test]
fn compositing_strategy_modifier_sets_graphics_layer_strategy() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier_nodes::GraphicsLayerNode;

    let modifier = Modifier::empty().compositing_strategy(CompositingStrategy::Offscreen);
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    let chain = handle.chain();
    let mut observed = None;
    chain.for_each_node_with_capability(
        cranpose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if let Some(layer_node) = node.as_any().downcast_ref::<GraphicsLayerNode>() {
                observed = Some(layer_node.layer().compositing_strategy);
            }
        },
    );

    assert_eq!(observed, Some(CompositingStrategy::Offscreen));
}

#[test]
fn layer_blend_mode_modifier_sets_graphics_layer_mode() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier_nodes::GraphicsLayerNode;

    let modifier = Modifier::empty().layer_blend_mode(BlendMode::DstOut);
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    let chain = handle.chain();
    let mut observed = None;
    chain.for_each_node_with_capability(
        cranpose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if let Some(layer_node) = node.as_any().downcast_ref::<GraphicsLayerNode>() {
                observed = Some(layer_node.layer().blend_mode);
            }
        },
    );

    assert_eq!(observed, Some(BlendMode::DstOut));
}

#[test]
fn blur_with_edge_treatment_sets_expected_render_effect() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier_nodes::GraphicsLayerNode;

    let modifier =
        Modifier::empty().blur_with_edge_treatment(Dp(6.0), BlurredEdgeTreatment::UNBOUNDED);
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    let chain = handle.chain();
    let mut observed = None;
    chain.for_each_node_with_capability(
        cranpose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if let Some(layer_node) = node.as_any().downcast_ref::<GraphicsLayerNode>() {
                observed = Some(layer_node.layer());
            }
        },
    );

    let layer = observed.expect("blur layer");
    let expected_radius = 6.0 * crate::current_density();
    assert_eq!(
        layer.render_effect,
        Some(RenderEffect::blur_with_edge_treatment(
            expected_radius,
            TileMode::Decal,
        ))
    );
    assert_eq!(layer.shape, LayerShape::Rectangle);
    assert!(!layer.clip);
}

#[test]
fn blur_with_default_edge_treatment_enables_rect_clip() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier_nodes::GraphicsLayerNode;

    let modifier = Modifier::empty().blur(Dp(0.0));
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    let chain = handle.chain();
    let mut observed = None;
    chain.for_each_node_with_capability(
        cranpose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if let Some(layer_node) = node.as_any().downcast_ref::<GraphicsLayerNode>() {
                observed = Some(layer_node.layer());
            }
        },
    );

    let layer = observed.expect("blur layer");
    assert_eq!(layer.render_effect, None);
    assert_eq!(layer.shape, LayerShape::Rectangle);
    assert!(layer.clip);
}

#[test]
fn blur_unbounded_zero_radius_is_noop() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier =
        Modifier::empty().blur_with_edge_treatment(Dp(0.0), BlurredEdgeTreatment::UNBOUNDED);
    let slices = super::collect_slices_from_modifier(&modifier);
    assert!(slices.graphics_layer().is_none());
}

#[test]
fn graphics_layer_params_sets_scale_axes_and_alpha() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier_nodes::GraphicsLayerNode;

    let modifier = Modifier::empty().graphics_layer_params(
        1.25,
        0.75,
        0.6,
        8.0,
        -3.0,
        5.0,
        2.0,
        4.0,
        6.0,
        10.0,
        TransformOrigin::new(0.25, 0.8),
        LayerShape::Rounded(RoundedCornerShape::uniform(12.0)),
        true,
        Some(RenderEffect::blur(4.0)),
        Color::from_rgba_u8(40, 60, 80, 255),
        Color::from_rgba_u8(120, 140, 160, 255),
        CompositingStrategy::Auto,
        BlendMode::SrcOver,
        Some(ColorFilter::tint(Color::from_rgba_u8(220, 200, 255, 128))),
    );
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    let chain = handle.chain();
    let mut observed = None;
    chain.for_each_node_with_capability(
        cranpose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if let Some(layer_node) = node.as_any().downcast_ref::<GraphicsLayerNode>() {
                observed = Some(layer_node.layer());
            }
        },
    );

    let layer = observed.expect("graphics layer");
    assert!((layer.scale_x - 1.25).abs() < 1e-6);
    assert!((layer.scale_y - 0.75).abs() < 1e-6);
    assert!((layer.alpha - 0.6).abs() < 1e-6);
    assert!((layer.translation_x - 8.0).abs() < 1e-6);
    assert!((layer.translation_y + 3.0).abs() < 1e-6);
    assert!((layer.shadow_elevation - 5.0).abs() < 1e-6);
    assert!((layer.rotation_x - 2.0).abs() < 1e-6);
    assert!((layer.rotation_y - 4.0).abs() < 1e-6);
    assert!((layer.rotation_z - 6.0).abs() < 1e-6);
    assert!((layer.camera_distance - 10.0).abs() < 1e-6);
    assert_eq!(layer.transform_origin, TransformOrigin::new(0.25, 0.8));
    assert_eq!(
        layer.shape,
        LayerShape::Rounded(RoundedCornerShape::uniform(12.0))
    );
    assert!(layer.clip);
    assert_eq!(
        layer.ambient_shadow_color,
        Color::from_rgba_u8(40, 60, 80, 255)
    );
    assert_eq!(
        layer.spot_shadow_color,
        Color::from_rgba_u8(120, 140, 160, 255)
    );
    assert!(layer.render_effect.is_some());
    assert!(layer.color_filter.is_some());
}

#[test]
fn graphics_layer_block_applies_configuration() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier_nodes::GraphicsLayerNode;

    let modifier = Modifier::empty().graphics_layer_block(|layer| {
        layer.alpha = 0.42;
        layer.scale_x = 1.4;
        layer.scale_y = 1.1;
        layer.translation_x = 12.0;
        layer.rotation_z = 18.0;
        layer.clip = true;
        layer.shape = LayerShape::Rounded(RoundedCornerShape::uniform(10.0));
        layer.transform_origin = TransformOrigin::new(0.3, 0.7);
    });
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    let chain = handle.chain();
    let mut observed = None;
    chain.for_each_node_with_capability(
        cranpose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if let Some(layer_node) = node.as_any().downcast_ref::<GraphicsLayerNode>() {
                observed = Some(layer_node.layer());
            }
        },
    );

    let layer = observed.expect("graphics layer");
    assert!((layer.alpha - 0.42).abs() < 1e-6);
    assert!((layer.scale_x - 1.4).abs() < 1e-6);
    assert!((layer.scale_y - 1.1).abs() < 1e-6);
    assert!((layer.translation_x - 12.0).abs() < 1e-6);
    assert!((layer.rotation_z - 18.0).abs() < 1e-6);
    assert!(layer.clip);
    assert_eq!(
        layer.shape,
        LayerShape::Rounded(RoundedCornerShape::uniform(10.0))
    );
    assert_eq!(layer.transform_origin, TransformOrigin::new(0.3, 0.7));
}

#[test]
fn graphics_layer_block_tracks_state_changes_without_recomposition() {
    let _app_context = crate::render_state::app_context_test_scope();
    let elevation = Rc::new(Cell::new(2.0f32));
    let modifier = Modifier::empty().graphics_layer_block({
        let elevation = elevation.clone();
        move |layer| {
            layer.shadow_elevation = elevation.get();
        }
    });

    let slices = super::collect_slices_from_modifier(&modifier);
    assert!((slices.graphics_layer().expect("layer").shadow_elevation - 2.0).abs() < 1e-6);

    elevation.set(9.0);
    assert!((slices.graphics_layer().expect("layer").shadow_elevation - 9.0).abs() < 1e-6);
}

#[test]
fn shadow_defaults_match_compose_behavior() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().shadow(6.0);
    let slices = super::collect_slices_from_modifier(&modifier);
    let layer = slices
        .graphics_layer()
        .expect("shadow should install layer");

    assert!((layer.shadow_elevation - 6.0).abs() < 1e-6);
    assert_eq!(layer.shape, LayerShape::Rectangle);
    assert!(layer.clip, "positive elevation defaults clip=true");
    assert_eq!(layer.ambient_shadow_color, Color::BLACK);
    assert_eq!(layer.spot_shadow_color, Color::BLACK);
}

#[test]
fn shadow_with_allows_zero_elevation_clipping_and_custom_colors() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().shadow_with(
        0.0,
        LayerShape::Rounded(RoundedCornerShape::uniform(11.0)),
        true,
        Color::from_rgba_u8(20, 30, 40, 200),
        Color::from_rgba_u8(60, 70, 80, 210),
    );
    let slices = super::collect_slices_from_modifier(&modifier);
    let layer = slices
        .graphics_layer()
        .expect("clip-only shadow should keep layer");

    assert!((layer.shadow_elevation - 0.0).abs() < 1e-6);
    assert_eq!(
        layer.shape,
        LayerShape::Rounded(RoundedCornerShape::uniform(11.0))
    );
    assert!(layer.clip);
    assert_eq!(
        layer.ambient_shadow_color,
        Color::from_rgba_u8(20, 30, 40, 200)
    );
    assert_eq!(
        layer.spot_shadow_color,
        Color::from_rgba_u8(60, 70, 80, 210)
    );
}

#[test]
fn shadow_with_zero_elevation_and_no_clip_is_noop() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().shadow_with(
        0.0,
        LayerShape::Rectangle,
        false,
        Color::BLACK,
        Color::BLACK,
    );
    let slices = super::collect_slices_from_modifier(&modifier);
    assert!(
        slices.graphics_layer().is_none(),
        "zero elevation + clip=false should not install a layer"
    );
}

fn primitive_rect(primitive: &DrawPrimitive) -> Option<cranpose_ui_graphics::Rect> {
    match primitive {
        DrawPrimitive::Rect { rect, .. }
        | DrawPrimitive::RoundRect { rect, .. }
        | DrawPrimitive::Image { rect, .. } => Some(*rect),
        DrawPrimitive::Blend { primitive, .. } => primitive_rect(primitive),
        DrawPrimitive::Shadow(ShadowPrimitive::Drop { shape, .. }) => primitive_rect(shape),
        DrawPrimitive::Shadow(ShadowPrimitive::Inner { fill, .. }) => primitive_rect(fill),
        DrawPrimitive::Content => None,
    }
}

fn contains_blend_mode(primitives: &[DrawPrimitive], mode: BlendMode) -> bool {
    primitives.iter().any(|primitive| match primitive {
        DrawPrimitive::Blend {
            primitive,
            blend_mode,
        } => *blend_mode == mode || contains_blend_mode(std::slice::from_ref(primitive), mode),
        DrawPrimitive::Shadow(ShadowPrimitive::Drop {
            shape, blend_mode, ..
        }) => *blend_mode == mode || contains_blend_mode(std::slice::from_ref(shape), mode),
        DrawPrimitive::Shadow(ShadowPrimitive::Inner {
            fill,
            cutout,
            blend_mode,
            ..
        }) => {
            *blend_mode == mode
                || mode == BlendMode::DstOut
                || contains_blend_mode(std::slice::from_ref(fill), mode)
                || contains_blend_mode(std::slice::from_ref(cutout), mode)
        }
        _ => false,
    })
}

fn brush_max_alpha(brush: &Brush) -> f32 {
    match brush {
        Brush::Solid(color) => color.a(),
        Brush::LinearGradient { colors, .. }
        | Brush::RadialGradient { colors, .. }
        | Brush::SweepGradient { colors, .. } => {
            colors.iter().map(|color| color.a()).fold(0.0f32, f32::max)
        }
    }
}

fn collect_dst_out_alphas(primitives: &[DrawPrimitive], out: &mut Vec<f32>) {
    for primitive in primitives {
        match primitive {
            DrawPrimitive::Blend {
                primitive,
                blend_mode,
            } => {
                if *blend_mode == BlendMode::DstOut {
                    match primitive.as_ref() {
                        DrawPrimitive::Rect { brush, .. }
                        | DrawPrimitive::RoundRect { brush, .. } => {
                            out.push(brush_max_alpha(brush));
                        }
                        _ => {}
                    }
                }
                collect_dst_out_alphas(std::slice::from_ref(primitive), out);
            }
            DrawPrimitive::Shadow(ShadowPrimitive::Drop { shape, .. }) => {
                collect_dst_out_alphas(std::slice::from_ref(shape), out);
            }
            DrawPrimitive::Shadow(ShadowPrimitive::Inner { fill, cutout, .. }) => {
                collect_dst_out_alphas(std::slice::from_ref(fill), out);
                match cutout.as_ref() {
                    DrawPrimitive::Rect { brush, .. } | DrawPrimitive::RoundRect { brush, .. } => {
                        out.push(brush_max_alpha(brush));
                    }
                    _ => collect_dst_out_alphas(std::slice::from_ref(cutout), out),
                }
            }
            _ => {}
        }
    }
}

fn max_primitive_width(primitives: &[DrawPrimitive]) -> f32 {
    primitives
        .iter()
        .filter_map(primitive_rect)
        .map(|rect| rect.width)
        .fold(0.0f32, f32::max)
}

fn first_inner_cutout_x(primitives: &[DrawPrimitive]) -> Option<f32> {
    primitives.iter().find_map(|primitive| match primitive {
        DrawPrimitive::Shadow(ShadowPrimitive::Inner { cutout, .. }) => {
            primitive_rect(cutout).map(|rect| rect.x)
        }
        DrawPrimitive::Blend { primitive, .. } => {
            first_inner_cutout_x(std::slice::from_ref(primitive))
        }
        _ => None,
    })
}

#[test]
fn drop_shadow_static_emits_behind_primitives() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().drop_shadow_value(
        LayerShape::Rounded(RoundedCornerShape::uniform(8.0)),
        Shadow {
            radius: Dp(12.0),
            spread: Dp(4.0),
            offset: DpOffset::new(Dp(6.0), Dp(3.0)),
            alpha: 0.8,
            ..Default::default()
        },
    );
    let slices = super::collect_slices_from_modifier(&modifier);
    let commands = slices.draw_commands();

    assert_eq!(commands.len(), 1);
    let DrawCommand::Behind(draw) = &commands[0] else {
        panic!("drop_shadow should emit a behind draw command");
    };
    let primitives = draw(Size {
        width: 60.0,
        height: 32.0,
    });
    assert!(
        !primitives.is_empty(),
        "drop shadow should produce visible primitives"
    );
    assert!(
        max_primitive_width(&primitives) > 60.0,
        "shadow footprint should expand beyond base bounds"
    );
}

#[test]
fn drop_shadow_closure_tracks_runtime_values() {
    let _app_context = crate::render_state::app_context_test_scope();
    let spread = Rc::new(Cell::new(0.0f32));
    let modifier = Modifier::empty().drop_shadow(LayerShape::Rectangle, {
        let spread = spread.clone();
        move |scope| {
            scope.radius = 10.0;
            scope.spread = spread.get();
        }
    });
    let slices = super::collect_slices_from_modifier(&modifier);
    let commands = slices.draw_commands();

    let DrawCommand::Behind(draw) = &commands[0] else {
        panic!("drop_shadow(block) should emit a behind draw command");
    };
    let before = draw(Size {
        width: 40.0,
        height: 24.0,
    });
    spread.set(18.0);
    let after = draw(Size {
        width: 40.0,
        height: 24.0,
    });

    assert!(
        max_primitive_width(&after) > max_primitive_width(&before),
        "increasing spread should increase shadow footprint"
    );
}

#[test]
fn drop_shadow_high_radius_emits_single_blurred_shadow_primitive() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().drop_shadow(LayerShape::Rectangle, |scope| {
        scope.radius = 40.0;
        scope.alpha = 0.85;
    });
    let slices = super::collect_slices_from_modifier(&modifier);
    let DrawCommand::Behind(draw) = &slices.draw_commands()[0] else {
        panic!("drop_shadow should emit a behind draw command");
    };
    let primitives = draw(Size {
        width: 80.0,
        height: 50.0,
    });
    assert_eq!(
        primitives.len(),
        1,
        "shadow API should emit one shadow primitive"
    );
    match &primitives[0] {
        DrawPrimitive::Shadow(ShadowPrimitive::Drop { blur_radius, .. }) => {
            assert!((*blur_radius - 40.0).abs() < f32::EPSILON);
        }
        other => panic!("expected drop shadow primitive, got {other:?}"),
    }
}

#[test]
fn inner_shadow_emits_overlay_with_dst_out_cutout() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().inner_shadow(LayerShape::Rectangle, |scope| {
        scope.radius = 14.0;
        scope.offset = Point::new(5.0, -3.0);
    });
    let slices = super::collect_slices_from_modifier(&modifier);
    let commands = slices.draw_commands();

    assert_eq!(commands.len(), 1);
    let DrawCommand::Overlay(draw) = &commands[0] else {
        panic!("inner_shadow should emit an overlay draw command");
    };
    let primitives = draw(Size {
        width: 64.0,
        height: 40.0,
    });

    assert!(
        contains_blend_mode(&primitives, BlendMode::DstOut),
        "inner shadow should carve interior with DstOut"
    );
}

#[test]
fn inner_shadow_large_radius_keeps_fill_and_cutout_pairs_balanced() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().inner_shadow(
        LayerShape::Rounded(RoundedCornerShape::uniform(10.0)),
        |scope| {
            scope.radius = 34.0;
            scope.spread = 14.0;
            scope.offset = Point::new(8.0, 6.0);
            scope.alpha = 0.9;
        },
    );
    let slices = super::collect_slices_from_modifier(&modifier);
    let DrawCommand::Overlay(draw) = &slices.draw_commands()[0] else {
        panic!("inner_shadow should emit an overlay draw command");
    };
    let primitives = draw(Size {
        width: 50.0,
        height: 42.0,
    });
    assert_eq!(
        primitives.len(),
        1,
        "inner shadow should emit one shadow primitive"
    );
    match &primitives[0] {
        DrawPrimitive::Shadow(ShadowPrimitive::Inner {
            fill,
            cutout,
            blur_radius,
            ..
        }) => {
            assert!((*blur_radius - 34.0).abs() < f32::EPSILON);
            let fill_rect = primitive_rect(fill).expect("inner fill should provide a shape rect");
            let cutout_rect =
                primitive_rect(cutout).expect("inner cutout should provide a shape rect");
            assert!(
                cutout_rect.width < fill_rect.width,
                "positive spread should shrink inner cutout geometry"
            );
            assert!(
                cutout_rect.height < fill_rect.height,
                "positive spread should shrink inner cutout geometry"
            );
        }
        other => panic!("expected inner shadow primitive, got {other:?}"),
    }
}

#[test]
fn inner_shadow_static_uses_density_for_dp_offset() {
    let _app_context = crate::render_state::app_context_test_scope();
    let _density = DensityGuard::set(2.0);

    let modifier = Modifier::empty().inner_shadow_value(
        LayerShape::Rectangle,
        Shadow {
            radius: Dp(0.0),
            spread: Dp(0.0),
            offset: DpOffset::new(Dp(2.0), Dp(0.0)),
            ..Default::default()
        },
    );
    let slices = super::collect_slices_from_modifier(&modifier);
    let commands = slices.draw_commands();
    let DrawCommand::Overlay(draw) = &commands[0] else {
        panic!("inner_shadow should emit an overlay draw command");
    };
    let primitives = draw(Size {
        width: 40.0,
        height: 20.0,
    });

    let cutout_x = first_inner_cutout_x(&primitives);
    assert_eq!(cutout_x, Some(4.0));
}

#[test]
fn inner_shadow_cutout_alpha_remains_opaque_for_hole_mask() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().inner_shadow(LayerShape::Rectangle, |scope| {
        scope.radius = 12.0;
        scope.offset = Point::new(4.0, 3.0);
        scope.alpha = 0.2;
    });
    let slices = super::collect_slices_from_modifier(&modifier);
    let DrawCommand::Overlay(draw) = &slices.draw_commands()[0] else {
        panic!("inner_shadow should emit an overlay draw command");
    };
    let primitives = draw(Size {
        width: 48.0,
        height: 30.0,
    });
    let mut dst_out_alphas = Vec::new();
    collect_dst_out_alphas(&primitives, &mut dst_out_alphas);

    assert!(
        !dst_out_alphas.is_empty(),
        "inner shadow should emit dst-out cutouts"
    );
    assert!(
        dst_out_alphas
            .iter()
            .all(|alpha| (*alpha - 1.0).abs() <= 1e-6),
        "dst-out cutout alpha should stay opaque so low alpha does not flood-fill the interior"
    );
}

#[test]
fn drop_shadow_emits_primitives() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().drop_shadow(LayerShape::Rectangle, |scope| {
        scope.radius = 8.0;
        scope.spread = 2.0;
    });
    let slices = super::collect_slices_from_modifier(&modifier);
    let DrawCommand::Behind(draw) = &slices.draw_commands()[0] else {
        panic!("drop_shadow should emit a behind draw command");
    };
    let primitives = draw(Size {
        width: 32.0,
        height: 18.0,
    });
    assert!(!primitives.is_empty(), "drop_shadow should draw");
}

#[test]
fn drop_shadow_value_alias_uses_static_shadow() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().drop_shadow_value(
        LayerShape::Rectangle,
        Shadow {
            radius: Dp(6.0),
            spread: Dp(2.0),
            alpha: 0.7,
            ..Default::default()
        },
    );
    let slices = super::collect_slices_from_modifier(&modifier);
    let DrawCommand::Behind(draw) = &slices.draw_commands()[0] else {
        panic!("drop_shadow_value alias should emit a behind draw command");
    };
    let primitives = draw(Size {
        width: 32.0,
        height: 18.0,
    });
    assert!(
        !primitives.is_empty(),
        "drop_shadow_value alias should draw"
    );
}

#[test]
fn inner_shadow_emits_primitives() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().inner_shadow(LayerShape::Rectangle, |scope| {
        scope.radius = 10.0;
        scope.offset = Point::new(3.0, 1.0);
    });
    let slices = super::collect_slices_from_modifier(&modifier);
    let DrawCommand::Overlay(draw) = &slices.draw_commands()[0] else {
        panic!("inner_shadow should emit an overlay draw command");
    };
    let primitives = draw(Size {
        width: 32.0,
        height: 18.0,
    });
    assert!(!primitives.is_empty(), "inner_shadow should draw");
}

#[test]
fn inner_shadow_value_alias_uses_static_shadow() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().inner_shadow_value(
        LayerShape::Rectangle,
        Shadow {
            radius: Dp(6.0),
            spread: Dp(2.0),
            offset: DpOffset::new(Dp(2.0), Dp(1.0)),
            ..Default::default()
        },
    );
    let slices = super::collect_slices_from_modifier(&modifier);
    let DrawCommand::Overlay(draw) = &slices.draw_commands()[0] else {
        panic!("inner_shadow_value alias should emit an overlay draw command");
    };
    let primitives = draw(Size {
        width: 32.0,
        height: 18.0,
    });
    assert!(
        !primitives.is_empty(),
        "inner_shadow_value alias should draw"
    );
}

#[test]
fn gradient_cut_mask_modifier_sets_render_effect_shader() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier_nodes::GraphicsLayerNode;

    let spec = GradientCutMaskSpec {
        progress: 0.5,
        feather: 20.0,
        corner_radius: 12.0,
        direction: CutDirection::TopToBottom,
    };
    let modifier = Modifier::empty().gradient_cut_mask(300.0, 180.0, spec);
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    let chain = handle.chain();
    let mut found_shader = false;
    chain.for_each_node_with_capability(
        cranpose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if let Some(layer_node) = node.as_any().downcast_ref::<GraphicsLayerNode>() {
                found_shader = matches!(
                    layer_node.layer().render_effect,
                    Some(RenderEffect::Shader { .. })
                );
            }
        },
    );

    assert!(
        found_shader,
        "Expected gradient_cut_mask to configure shader"
    );
}

#[test]
fn rounded_alpha_mask_modifier_sets_render_effect_shader() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier_nodes::GraphicsLayerNode;

    let modifier = Modifier::empty().rounded_alpha_mask(280.0, 120.0, 14.0, 8.0);
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    let chain = handle.chain();
    let mut found_shader = false;
    chain.for_each_node_with_capability(
        cranpose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if let Some(layer_node) = node.as_any().downcast_ref::<GraphicsLayerNode>() {
                found_shader = matches!(
                    layer_node.layer().render_effect,
                    Some(RenderEffect::Shader { .. })
                );
            }
        },
    );

    assert!(
        found_shader,
        "Expected rounded_alpha_mask to configure shader"
    );
}

#[test]
fn rounded_alpha_mask_identity_is_noop() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier_nodes::GraphicsLayerNode;

    let modifier = Modifier::empty().rounded_alpha_mask(280.0, 120.0, 0.0, 0.0);
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    let chain = handle.chain();
    let mut found_layer = false;
    chain.for_each_node_with_capability(
        cranpose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if node.as_any().downcast_ref::<GraphicsLayerNode>().is_some() {
                found_layer = true;
            }
        },
    );

    assert!(
        !found_layer,
        "identity rounded_alpha_mask should not add a layer"
    );
}

#[test]
fn rounded_corners_records_corner_shape_without_background() {
    let _app_context = crate::render_state::app_context_test_scope();
    let shape = RoundedCornerShape::new(4.0, 8.0, 12.0, 16.0);
    let modifier = Modifier::empty().rounded_corner_shape(shape);
    let slices = collect_slices_from_modifier(&modifier);

    assert_eq!(slices.corner_shape(), Some(shape));
}

#[test]
fn gradient_fade_dst_out_modifier_sets_render_effect_shader() {
    let _app_context = crate::render_state::app_context_test_scope();
    use crate::modifier_nodes::GraphicsLayerNode;

    let spec = GradientFadeMaskSpec {
        start: 18.0,
        end: 42.0,
        direction: CutDirection::TopToBottom,
    };
    let modifier = Modifier::empty().gradient_fade_dst_out(260.0, 120.0, spec);
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);

    let chain = handle.chain();
    let mut found_shader = false;
    chain.for_each_node_with_capability(
        cranpose_foundation::NodeCapabilities::DRAW,
        |_ref, node| {
            if let Some(layer_node) = node.as_any().downcast_ref::<GraphicsLayerNode>() {
                found_shader = matches!(
                    layer_node.layer().render_effect,
                    Some(RenderEffect::Shader { .. })
                );
            }
        },
    );

    assert!(
        found_shader,
        "Expected gradient_fade_dst_out to configure shader"
    );
}

#[test]
fn collect_inspector_records_include_weight_and_pointer_input_metadata() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty()
        .padding(2.0)
        .then(Modifier::empty().weight_with_fill(3.5, false))
        .then(Modifier::empty().pointer_input(7u64, |_| async move {}));

    let records = modifier.collect_inspector_records();

    let weight = records
        .iter()
        .find(|record| record.name == "weight")
        .expect("missing weight inspector record");
    assert!(weight
        .properties
        .iter()
        .any(|prop| prop.name == "weight" && prop.value == 3.5f32.to_string()));
    assert!(weight
        .properties
        .iter()
        .any(|prop| prop.name == "fill" && prop.value == "false"));

    let pointer = records
        .iter()
        .find(|record| record.name == "pointerInput")
        .expect("missing pointerInput inspector record");
    assert!(pointer
        .properties
        .iter()
        .any(|prop| prop.name == "keyCount" && prop.value == "1"));
    assert!(pointer
        .properties
        .iter()
        .any(|prop| prop.name == "handlerId"));
}

#[test]
fn semantics_modifier_populates_inspector_metadata() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().semantics(|config: &mut SemanticsConfiguration| {
        config.content_description = Some("Submit".into());
        config.is_button = true;
    });

    let records = modifier.collect_inspector_records();
    let semantics = records
        .first()
        .expect("expected semantics inspector record");
    assert_eq!(semantics.name, "semantics");
    assert!(semantics
        .properties
        .iter()
        .any(|prop| prop.name == "contentDescription" && prop.value == "Submit"));
    assert!(semantics
        .properties
        .iter()
        .any(|prop| prop.name == "isButton" && prop.value == "true"));
}

#[test]
fn inspector_snapshot_includes_delegate_depth_and_capabilities() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().padding(4.0).then(
        Modifier::with_element(TestDelegatingElement)
            .with_inspector_metadata(inspector_metadata("delegating", |info| {
                info.add_property("tag", "root")
            })),
    );
    let mut handle = ModifierChainHandle::new();
    let _ = handle.update(&modifier);
    // Use test-only method to populate snapshot without triggering global trace
    handle.refresh_inspector_snapshot(&modifier);

    let snapshot = handle.inspector_snapshot();
    assert!(snapshot.iter().any(|node| node.depth > 0));
    let padding_entry = snapshot
        .iter()
        .find(|node| {
            node.inspector
                .as_ref()
                .map(|record| record.name == "padding")
                .unwrap_or(false)
        })
        .expect("expected padding inspector entry");
    assert!(padding_entry
        .capabilities
        .contains(NodeCapabilities::LAYOUT));
}

#[test]
fn modifier_chain_trace_runs_only_when_debug_flag_set() {
    let _app_context = crate::render_state::app_context_test_scope();
    let modifier = Modifier::empty().padding(1.0);
    let mut handle = ModifierChainHandle::new();
    let invocations = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    {
        let counter = invocations.clone();
        let _guard = crate::debug::install_modifier_chain_trace(move |_nodes| {
            *counter.lock().unwrap() += 1;
        });

        let _ = handle.update(&modifier);
        assert_eq!(
            *invocations.lock().unwrap(),
            0,
            "trace should be gated by debug flag"
        );

        handle.set_debug_logging(true);
        let _ = handle.update(&modifier);
    }

    assert_eq!(*invocations.lock().unwrap(), 1);
}

#[test]
fn modifier_chain_trace_is_app_context_owned() {
    let first_context = crate::render_state::AppContext::new();
    let second_context = crate::render_state::AppContext::new();
    let first_invocations = std::sync::Arc::new(std::sync::Mutex::new(0usize));
    let second_invocations = std::sync::Arc::new(std::sync::Mutex::new(0usize));

    let _first_guard = first_context.enter(|| {
        let invocations = first_invocations.clone();
        crate::debug::install_modifier_chain_trace(move |_nodes| {
            *invocations.lock().unwrap() += 1;
        })
    });
    let _second_guard = second_context.enter(|| {
        let invocations = second_invocations.clone();
        crate::debug::install_modifier_chain_trace(move |_nodes| {
            *invocations.lock().unwrap() += 1;
        })
    });

    let modifier = Modifier::empty().padding(1.0);
    let mut first_handle = ModifierChainHandle::new();
    first_handle.set_debug_logging(true);
    let mut second_handle = ModifierChainHandle::new();
    second_handle.set_debug_logging(true);

    first_context.enter(|| {
        let _ = first_handle.update(&modifier);
    });
    second_context.enter(|| {
        let _ = second_handle.update(&modifier);
    });

    assert_eq!(*first_invocations.lock().unwrap(), 1);
    assert_eq!(*second_invocations.lock().unwrap(), 1);
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TestFingerprintElement {
    id: u32,
    capabilities: NodeCapabilities,
    always_update: bool,
}

struct TestFingerprintNode {
    state: NodeState,
}

impl TestFingerprintNode {
    fn new(capabilities: NodeCapabilities) -> Self {
        let node = Self {
            state: NodeState::new(),
        };
        node.state.set_capabilities(capabilities);
        node
    }
}

impl DelegatableNode for TestFingerprintNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for TestFingerprintNode {}

impl ModifierNodeElement for TestFingerprintElement {
    type Node = TestFingerprintNode;

    fn create(&self) -> Self::Node {
        TestFingerprintNode::new(self.capabilities)
    }

    fn update(&self, _node: &mut Self::Node) {}

    fn capabilities(&self) -> NodeCapabilities {
        self.capabilities
    }

    fn always_update(&self) -> bool {
        self.always_update
    }
}

fn test_fingerprint_element(
    id: u32,
    capabilities: NodeCapabilities,
    always_update: bool,
) -> DynModifierElement {
    modifier_element(TestFingerprintElement {
        id,
        capabilities,
        always_update,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TestDelegatingElement;

struct TestDelegatingNode {
    state: NodeState,
    delegate: TestDelegateLeaf,
}

impl TestDelegatingNode {
    fn new() -> Self {
        let node = Self {
            state: NodeState::new(),
            delegate: TestDelegateLeaf::new(),
        };
        node.state
            .set_capabilities(NodeCapabilities::LAYOUT | NodeCapabilities::MODIFIER_LOCALS);
        node.delegate
            .node_state()
            .set_capabilities(NodeCapabilities::LAYOUT);
        node
    }
}

impl DelegatableNode for TestDelegatingNode {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for TestDelegatingNode {
    fn for_each_delegate<'a>(&'a self, visitor: &mut dyn FnMut(&'a dyn ModifierNode)) {
        visitor(&self.delegate);
    }
}

struct TestDelegateLeaf {
    state: NodeState,
}

impl TestDelegateLeaf {
    fn new() -> Self {
        Self {
            state: NodeState::new(),
        }
    }
}

impl DelegatableNode for TestDelegateLeaf {
    fn node_state(&self) -> &NodeState {
        &self.state
    }
}

impl ModifierNode for TestDelegateLeaf {}

impl ModifierNodeElement for TestDelegatingElement {
    type Node = TestDelegatingNode;

    fn create(&self) -> Self::Node {
        TestDelegatingNode::new()
    }

    fn update(&self, _node: &mut Self::Node) {}

    fn capabilities(&self) -> NodeCapabilities {
        NodeCapabilities::LAYOUT
    }
}
