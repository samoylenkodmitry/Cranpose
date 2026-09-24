use super::*;

#[test]
fn backdrop_blur_validates_radius_and_opacity() {
    for (radius, opacity, expected) in [
        (2.0, 0.5, Some((2.0, 0.5))),
        (-2.0, 3.0, Some((0.0, 1.0))),
        (2.0, -1.0, Some((2.0, 0.0))),
        (f32::NAN, 1.0, None),
        (2.0, f32::INFINITY, None),
    ] {
        assert_eq!(
            Glass::lens().backdrop_blur(radius, opacity).backdrop_blur,
            expected
        );
    }
    let glass = Glass::lens()
        .backdrop_blur(2.0, 0.75)
        .edge_refraction(0.0)
        .resolve(&light_colors());
    let shader = terminal_shader(glass.backdrop_effect(3.0, GlassDynamics::default()));
    assert_eq!(shader.uniforms()[172..174], [6.0, 0.75]);
    let mask = terminal_shader(glass.runtime_effect(3.0, &GlassDynamics::default(), true));
    assert_eq!(
        mask.uniforms().get(172..174).unwrap_or(&[0.0, 0.0]),
        [0.0, 0.0]
    );
}

#[test]
fn pane_tint_preserves_the_sharp_source_at_every_percentage() {
    for percent in [0.0, 25.0, 50.0, 100.0] {
        let amount = GlassTintAmount::from_percent(percent).unwrap();
        let glass = Glass::regular()
            .adaptive_frost(Color::BLACK, 0.0)
            .tint_amount(amount);
        assert_eq!(glass.tint_amount, Some(amount));
        let effect = glass.backdrop_effect(&light_colors(), 3.0, GlassDynamics::default());
        let RenderEffect::Shader { shader } = effect else {
            panic!("the sharp backdrop must not be destroyed by a preceding blur");
        };
        assert_eq!(
            shader.uniforms()[cranpose_ui_graphics::GLASS_PANE_BLEND_UNIFORM],
            24.0
        );
        assert_eq!(
            shader.uniforms()[cranpose_ui_graphics::GLASS_PANE_BLEND_UNIFORM + 1],
            percent / 100.0
        );
        assert_eq!(
            shader.substrates(),
            &[cranpose_ui_graphics::SubstrateSpec::Blur { radius_px: 24.0 }]
        );
    }
}

fn light_colors() -> LiquidColors {
    LiquidColors::light(Color::from_rgb_u8(0, 122, 255))
}

fn terminal_shader(effect: RenderEffect) -> RuntimeShader {
    match effect {
        RenderEffect::Shader { shader } => std::sync::Arc::unwrap_or_clone(shader),
        RenderEffect::Chain { second, .. } => {
            terminal_shader(std::sync::Arc::unwrap_or_clone(second))
        }
        effect => panic!("expected runtime shader, got {effect:?}"),
    }
}

#[test]
fn optical_projection_validates_scales_and_projects_output_support() {
    let glass = Glass::lens().resolve(&light_colors());
    let mut dynamics = GlassDynamics {
        morph: Some(GlassMorph {
            node_size: (200.0, 120.0),
            primary: (100.0, 60.0, 111.0, 70.0, 35.0),
            ..Default::default()
        }),
        ..Default::default()
    };
    let base = terminal_shader(glass.backdrop_effect(3.0, dynamics.clone()));
    for projection in [
        None,
        Some((0.0, 1.0)),
        Some((-1.0, 1.0)),
        Some((1.0, f32::NAN)),
        Some((f32::INFINITY, 1.0)),
    ] {
        dynamics.optical_projection = projection;
        let shader = terminal_shader(glass.backdrop_effect(3.0, dynamics.clone()));
        assert_eq!(
            &shader.uniforms()
                [GLASS_OPTICAL_PROJECTION_UNIFORM..GLASS_OPTICAL_PROJECTION_UNIFORM + 2],
            &[1.0, 1.0]
        );
        assert_eq!(shader.output_support(), base.output_support());
    }
    dynamics.optical_projection = Some((1.2, 0.8));
    let projected = terminal_shader(glass.backdrop_effect(3.0, dynamics));
    assert_eq!(
        &projected.uniforms()
            [GLASS_OPTICAL_PROJECTION_UNIFORM..GLASS_OPTICAL_PROJECTION_UNIFORM + 2],
        &[1.2, 0.8]
    );
    let a = base.output_support().unwrap();
    let b = projected.output_support().unwrap();
    assert!((b.width - a.width * 1.2).abs() < 0.001);
    assert!((b.height - a.height * 0.8).abs() < 0.001);
}

#[test]
fn content_layers_preserve_the_complete_material_effect_and_cache() {
    let resolved = Glass::lens()
        .edge_refraction(9.0)
        .no_clip()
        .resolve(&light_colors());
    let layers = cached_glass_content_layers(resolved.clone());
    for density in [1.0, 3.0] {
        for activity in [0.0, 0.5, 1.0] {
            let dynamics = GlassDynamics {
                activity: Some(activity),
                ..Default::default()
            };
            let actual = layers(density, dynamics.clone());
            let joined = actual[0]
                .backdrop_effect
                .clone()
                .unwrap()
                .then(actual[1].backdrop_effect.clone().unwrap());
            assert_eq!(
                joined,
                resolved.runtime_effect_with_content(density, &dynamics, false, true)
            );
            assert!(actual[0].render_effect.is_none());
            assert!(!actual[0].clip);
            assert_eq!(
                terminal_shader(actual[1].backdrop_effect.clone().unwrap()).uniforms()[147],
                3.0
            );
            let mut repeated = layers(density, dynamics.clone());
            assert_eq!(actual, repeated);
            repeated[0].backdrop_effect = None;
            assert_eq!(actual, layers(density, dynamics));
        }
    }
}

#[test]
fn cached_glass_layers_refresh_all_live_inputs_and_keep_clones_independent() {
    let resolved = Glass::regular().no_clip().resolve(&light_colors());
    let layer = cached_glass_layer(resolved.clone());
    let mut dynamics = GlassDynamics::default();
    let mut density = 2.0;
    let mut previous = None;
    for step in 0..16 {
        match step {
            1 => density = 3.0,
            2 => set_glass_light_direction((0.5, -0.5)),
            3 => dynamics.highlight_boost = 0.25,
            4 => dynamics.saturation_boost = 0.4,
            5 => dynamics.activity = Some(0.5),
            6 => dynamics.resting_tint = Some(Color::RED),
            7 => dynamics.tint_alpha_multiplier = Some(0.3),
            8 => dynamics.touch = Some((5.0, 8.0, 0.7)),
            9 => dynamics.press_depth = Some(0.4),
            10 => {
                dynamics.morph = Some(GlassMorph {
                    node_size: (60.0, 30.0),
                    primary: (30.0, 15.0, 50.0, 20.0, 4.0),
                    shapes: vec![(10.0, 15.0, 8.0, 8.0, -1.0)],
                    ..Default::default()
                });
            }
            11 => {
                let morph = dynamics.morph.as_mut().unwrap();
                morph.primary.0 += 3.0;
                morph.shapes[0].0 += 2.0;
                morph.deformation = Some(GlassDeformation::incompressible((1.0, 1.0), 1.3));
            }
            12 => dynamics.morph = None,
            13 => dynamics.touch_radius_dp = Some(144.0),
            14 => dynamics.edge_return_depth_dp = Some(3.5),
            15 => dynamics.optical_projection = Some((1.2, 0.8)),
            _ => {}
        }
        let actual = layer(density, dynamics.clone());
        let expected = GraphicsLayer {
            backdrop_effect: Some(resolved.backdrop_effect(density, dynamics.clone())),
            render_effect: dynamics
                .morph
                .as_ref()
                .map(|_| resolved.runtime_effect(density, &dynamics, true)),
            shape: resolved.shape.layer_shape(),
            clip: resolved.clip,
            ..Default::default()
        };
        assert_eq!(actual, expected, "live input step {step}");
        assert_ne!(
            previous.as_ref(),
            Some(&actual),
            "input change must reach the layer at step {step}"
        );
        let mut repeated = layer(density, dynamics.clone());
        assert_eq!(repeated, actual);
        repeated.backdrop_effect = None;
        assert_eq!(layer(density, dynamics.clone()), actual);
        previous = Some(actual);
    }
    set_glass_light_direction((0.0, 1.0));
    let replacement = cached_glass_layer(
        Glass::clear()
            .shape(LiquidShape::RoundedRect(9.0))
            .resolve(&light_colors()),
    );
    assert_ne!(
        replacement(density, dynamics.clone()),
        layer(density, dynamics)
    );
}

#[test]
fn glass_cache_input_equality_preserves_signed_zero_and_nan_bits() {
    let mut a = GlassDynamics {
        touch: Some((0.0, 1.0, 0.5)),
        ..Default::default()
    };
    let mut b = a.clone();
    b.touch.as_mut().unwrap().0 = -0.0;
    assert!(!glass_dynamics_match(&a, &b));
    a.highlight_boost = f32::NAN;
    assert!(glass_dynamics_match(&a, &a));
    a.morph = Some(GlassMorph::default());
    b = a.clone();
    b.morph.as_mut().unwrap().zoom_anchor.0 = -0.0;
    assert!(!glass_dynamics_match(&a, &b));
    b = a.clone();
    b.morph
        .as_mut()
        .unwrap()
        .shapes
        .push((0.0, 1.0, 2.0, 3.0, 4.0));
    assert!(!glass_dynamics_match(&a, &b));
}

#[test]
fn glass_light_direction_defaults_overhead_and_reaches_the_shader() {
    assert_eq!(glass_light_direction(), (0.0, 1.0));
    let resolved = Glass::regular().resolve(&light_colors());
    let effect = resolved.backdrop_effect(2.0, GlassDynamics::default());
    let shader = terminal_shader(effect);
    let u = shader.uniforms();
    assert_eq!(u[GLASS_LIGHT_DIRECTION_UNIFORM], 0.0);
    assert_eq!(u[GLASS_LIGHT_DIRECTION_UNIFORM + 1], 1.0);

    set_glass_light_direction((1.0, 0.0));
    let effect = resolved.backdrop_effect(2.0, GlassDynamics::default());
    let u_rotated = terminal_shader(effect);
    let u_rotated = u_rotated.uniforms();
    assert_eq!(u_rotated[GLASS_LIGHT_DIRECTION_UNIFORM], 1.0);
    assert_eq!(u_rotated[GLASS_LIGHT_DIRECTION_UNIFORM + 1], 0.0);
    set_glass_light_direction((0.0, 1.0));
}

#[test]
fn liquid_shape_builds_matching_clip_and_layer_shapes() {
    for shape in [
        LiquidShape::Capsule,
        LiquidShape::Circle,
        LiquidShape::RoundedRect(12.0),
    ] {
        assert_eq!(shape.layer_shape(), LayerShape::Rounded(shape.clip_shape()));
    }
}

#[test]
fn glass_shadow_clamps_negative_radius() {
    let shadow = GlassShadow::new(Color::BLACK, -2.0, 3.0, -1.0);
    assert_eq!(shadow.radius, 0.0);
    assert_eq!(shadow.offset_y, 3.0);
    assert_eq!(shadow.spread, -1.0);
}

#[test]
fn incompressible_deformation_normalizes_axis_and_conserves_area() {
    let deformation = GlassDeformation::incompressible((3.0, 4.0), 1.25);
    assert_eq!(deformation.axis(), (0.6, 0.8));
    assert_eq!(deformation.along(), 1.25);
    assert!((deformation.along() * deformation.across() - 1.0).abs() < 1.0e-6);
    assert_eq!(
        GlassDeformation::incompressible((0.0, 0.0), 0.0).axis(),
        (1.0, 0.0)
    );
}

#[test]
fn glass_builders_clamp_physical_inputs() {
    let glass = Glass::lens()
        .shape(LiquidShape::Circle)
        .tint(Color::BLACK)
        .blur_radius(-2.0)
        .saturation(1.2)
        .refraction_depth(3.0)
        .refraction_depth_dp(-3.0)
        .refraction_curve(2.0)
        .dispersion(2.0)
        .transmission_refraction(2.0)
        .meniscus_absorption(2.0)
        .highlight(0.4)
        .lift(-0.2)
        .adaptive_frost(Color::WHITE, 2.0)
        .shadow(false)
        .no_clip();
    assert_eq!(glass.shape, LiquidShape::Circle);
    assert_eq!(glass.tint, Some(Color::BLACK));
    assert_eq!(glass.blur_radius, Some(-2.0));
    assert_eq!(glass.saturation, Some(1.2));
    assert_eq!(glass.refraction_depth, 2.0);
    assert_eq!(glass.refraction_depth_dp, Some(0.0));
    assert_eq!(glass.refraction_curve, 1.0);
    assert_eq!(glass.dispersion, 1.0);
    assert_eq!(glass.transmission_refraction, 1.0);
    assert_eq!(glass.meniscus_absorption, 1.0);
    assert_eq!(glass.highlight, 0.4);
    assert_eq!(glass.lift, Some(-0.2));
    assert_eq!(glass.adaptive_frost, 1.0);
    assert!(!glass.shadow);
    assert!(!glass.clip);
}

#[test]
fn material_variants_resolve_distinct_frost_levels() {
    let regular = Glass::regular().resolve(&light_colors());
    let clear = Glass::clear().resolve(&light_colors());
    let lens = Glass::lens().resolve(&light_colors());
    assert!(regular.blur_radius_dp > clear.blur_radius_dp);
    assert!(clear.blur_radius_dp > lens.blur_radius_dp);
    assert!(regular.saturation > clear.saturation);
    assert_eq!(lens.rim_style, 1.0);
    assert_eq!(regular.refraction_depth, 0.0);
    assert_eq!(regular.transmission_refraction, 0.0);
    assert_eq!(regular.refraction_curve, 0.25);
    assert_eq!(lens.refraction_curve, 1.0);
    assert_eq!(regular.dispersion, 0.0);
    assert_eq!(lens.dispersion, 0.30);
}

#[test]
fn neutral_surface_helpers_follow_foreground_polarity() {
    assert_eq!(
        neutral_surface_tint(Color::BLACK, 0.08, 0.10),
        Color::BLACK.with_alpha(0.08)
    );
    assert_eq!(
        neutral_surface_tint(Color::WHITE, 0.08, 0.10),
        Color::WHITE.with_alpha(0.10)
    );
}

#[test]
fn dynamic_saturation_reaches_the_material_tint() {
    let resting = Color::from_rgb_u8(0, 199, 208);
    let raised = boost_tint_saturation(resting, 0.55);
    assert_eq!(raised.r(), 0.0);
    assert!(raised.g() > resting.g());
    assert!(raised.b() > resting.b());
    assert_eq!(raised.a(), resting.a());
}

#[test]
fn template_specialization_follows_each_materials_dispersion_and_activity() {
    for (dispersion, activity) in [(0.6, 0.0), (0.0, 0.5), (0.3, 1.0), (0.0, 0.0)] {
        let resolved = Glass::regular()
            .dispersion(dispersion)
            .resolve(&light_colors());
        let mut shader = terminal_shader(resolved.backdrop_effect(
            1.0,
            GlassDynamics {
                activity: Some(activity),
                ..GlassDynamics::default()
            },
        ));
        cranpose_ui_graphics::specialize_liquid_glass_with_folds(&mut shader, true);
        assert_eq!(
            shader.uniforms()[GLASS_DISPERSION_UNIFORM],
            dispersion * activity
        );
        assert_eq!(shader.uniforms()[GLASS_ACTIVITY_UNIFORM], activity);
        assert_eq!(
            shader
                .overrides()
                .iter()
                .any(|(name, _)| { *name == cranpose_ui_graphics::GLASS_DISPERSION_OFF_FLAG }),
            dispersion * activity == 0.0
        );
    }
}

fn spectrum_fixture() -> GlassSpectrum {
    GlassSpectrum {
        angle_radians: 0.7,
        step_dp: -1.5,
        vertical_scale: 1.25,
        opacity_near: 0.25,
        opacity_far: 0.75,
        fade_depth_dp: 3.5,
        extent_dp: 12.0,
    }
}

#[test]
fn edge_spectrum_preserves_signed_taps_and_captures_every_ray() {
    let spectrum = spectrum_fixture();
    let material = Glass::lens()
        .edge_refraction(9.0)
        .dispersion(0.0)
        .edge_spectrum(spectrum);
    assert_eq!(material.edge_spectrum, Some(spectrum));
    let resolved = material.resolve(&light_colors());
    let shader = terminal_shader(resolved.backdrop_effect(3.0, GlassDynamics::default()));
    assert_eq!(
        &shader.uniforms()[GLASS_EDGE_SPECTRUM_UNIFORM..GLASS_EDGE_SPECTRUM_UNIFORM + 8],
        &spectrum.uniforms().unwrap()
    );
    let ordinary = Glass::lens()
        .edge_refraction(9.0)
        .dispersion(0.0)
        .resolve(&light_colors());
    let base = terminal_shader(ordinary.backdrop_effect(3.0, GlassDynamics::default()));
    assert!((shader.input_padding() - base.input_padding() - 5.625).abs() < 0.001);
}

#[test]
fn edge_spectrum_rejects_invalid_fields_at_both_entry_points() {
    for index in 0..7 {
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut spectrum = spectrum_fixture();
            let fields = [
                &mut spectrum.angle_radians,
                &mut spectrum.step_dp,
                &mut spectrum.vertical_scale,
                &mut spectrum.opacity_near,
                &mut spectrum.opacity_far,
                &mut spectrum.fade_depth_dp,
                &mut spectrum.extent_dp,
            ];
            *fields.into_iter().nth(index).unwrap() = invalid;
            assert_eq!(Glass::lens().edge_spectrum(spectrum).edge_spectrum, None);
            let material = Glass {
                edge_spectrum: Some(spectrum),
                ..Glass::lens()
            };
            let shader = terminal_shader(
                material
                    .resolve(&light_colors())
                    .backdrop_effect(1.0, GlassDynamics::default()),
            );
            assert_eq!(
                shader
                    .uniforms()
                    .get(GLASS_EDGE_SPECTRUM_UNIFORM + 7)
                    .copied()
                    .unwrap_or(0.0),
                0.0
            );
        }
    }
    for index in 2..7 {
        let mut spectrum = spectrum_fixture();
        let fields = [
            &mut spectrum.vertical_scale,
            &mut spectrum.opacity_near,
            &mut spectrum.opacity_far,
            &mut spectrum.fade_depth_dp,
            &mut spectrum.extent_dp,
        ];
        *fields.into_iter().nth(index - 2).unwrap() = -0.1;
        assert_eq!(Glass::lens().edge_spectrum(spectrum).edge_spectrum, None);
    }
    for near in [true, false] {
        let mut spectrum = spectrum_fixture();
        if near {
            spectrum.opacity_near = 1.1;
        } else {
            spectrum.opacity_far = 1.1;
        }
        assert_eq!(Glass::lens().edge_spectrum(spectrum).edge_spectrum, None);
    }
    let zero = GlassSpectrum {
        angle_radians: 0.0,
        step_dp: 0.0,
        vertical_scale: 0.0,
        opacity_near: 0.0,
        opacity_far: 1.0,
        fade_depth_dp: 0.0,
        extent_dp: 0.0,
    };
    assert_eq!(Glass::lens().edge_spectrum(zero).edge_spectrum, Some(zero));
}

#[test]
fn material_template_preserves_each_instances_optical_zoom_anchor() {
    let resolved = Glass::lens().resolve(&light_colors());
    for anchor in [(3.5, -2.25), (0.0, 0.0), (-7.0, 11.0)] {
        let shader = terminal_shader(resolved.backdrop_effect(
            1.5,
            GlassDynamics {
                morph: Some(GlassMorph {
                    node_size: (120.0, 72.0),
                    primary: (60.0, 36.0, 96.0, 52.0, -1.0),
                    zoom_anchor: anchor,
                    ..Default::default()
                }),
                ..Default::default()
            },
        ));
        assert_eq!(
            &shader.uniforms()
                [GLASS_OPTICAL_ZOOM_ANCHOR_UNIFORM..GLASS_OPTICAL_ZOOM_ANCHOR_UNIFORM + 2],
            &[anchor.0, anchor.1]
        );
    }
}

#[test]
fn key_fill_preserves_valid_parameters_and_rejects_invalid_fields() {
    let lighting = GlassKeyFill {
        height_dp: 1.0,
        curvature: 0.8,
        angle_radians: -std::f32::consts::FRAC_PI_4,
        saturation: 1.5,
        luma_gain: 0.4367,
        offset: 1.125,
        scale_with_surface: true,
    };
    for glass in [Glass::regular(), Glass::clear(), Glass::lens()] {
        assert!(glass.key_fill.is_none());
        let material = glass.key_fill(lighting);
        assert_eq!(material.key_fill, Some(lighting));
        let shader = terminal_shader(material.backdrop_effect(
            &light_colors(),
            3.0,
            GlassDynamics::default(),
        ));
        assert_eq!(
            &shader.uniforms()[GLASS_KEY_FILL_UNIFORM..GLASS_KEY_FILL_UNIFORM + 7],
            &lighting.uniforms().unwrap()
        );
    }
    for scale_with_surface in [false, true] {
        let value = GlassKeyFill {
            scale_with_surface,
            ..lighting
        };
        assert_eq!(value.uniforms().unwrap()[6], f32::from(scale_with_surface));
        let layer = cached_glass_layer(Glass::clear().key_fill(value).resolve(&light_colors()))(
            3.0,
            GlassDynamics::default(),
        );
        assert!(!layer.clip);
        assert!(layer.render_effect.is_some());
        assert!(layer.backdrop_effect.is_some());
    }
    for index in 0..6 {
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut value = lighting;
            match index {
                0 => value.height_dp = invalid,
                1 => value.curvature = invalid,
                2 => value.angle_radians = invalid,
                3 => value.saturation = invalid,
                4 => value.luma_gain = invalid,
                _ => value.offset = invalid,
            }
            let shader = terminal_shader(Glass::clear().key_fill(value).backdrop_effect(
                &light_colors(),
                3.0,
                GlassDynamics::default(),
            ));
            assert_eq!(
                shader
                    .uniforms()
                    .get(GLASS_KEY_FILL_UNIFORM)
                    .copied()
                    .unwrap_or_default(),
                0.0
            );
        }
    }
    for invalid in [0.0, -1.0] {
        assert!(
            GlassKeyFill {
                height_dp: invalid,
                ..lighting
            }
            .uniforms()
            .is_none()
        );
        assert!(
            GlassKeyFill {
                curvature: invalid,
                ..lighting
            }
            .uniforms()
            .is_none()
        );
    }
}

#[test]
fn independent_edge_return_depth_preserves_zero_and_rejects_invalid_values() {
    for depth in [
        None,
        Some(0.0),
        Some(3.5),
        Some(-1.0),
        Some(f32::NAN),
        Some(f32::INFINITY),
    ] {
        let shader = terminal_shader(
            Glass::lens()
                .edge_refraction(9.0)
                .refraction_depth_dp(36.0)
                .backdrop_effect(
                    &light_colors(),
                    3.0,
                    GlassDynamics {
                        edge_return_depth_dp: depth,
                        ..GlassDynamics::default()
                    },
                ),
        );
        let valid = depth.filter(|value| value.is_finite() && *value >= 0.0);
        assert_eq!(
            shader
                .uniforms()
                .get(GLASS_EDGE_RETURN_DEPTH_UNIFORM)
                .copied()
                .unwrap_or(0.0),
            valid.unwrap_or(0.0)
        );
        assert_eq!(
            shader
                .uniforms()
                .get(GLASS_EDGE_RETURN_DEPTH_UNIFORM + 1)
                .copied()
                .unwrap_or(0.0),
            f32::from(valid.is_some())
        );
        assert_eq!(
            shader.uniforms()[GLASS_PHYSICAL_REFRACTION_DEPTH_UNIFORM],
            36.0
        );
    }
}

#[test]
fn inner_shadow_validates_and_resolves_its_gaussian_profile() {
    let shadow = GlassShadow::new(Color::BLACK.with_alpha(0.12), 3.0, 7.0, 0.0);
    for glass in [Glass::regular(), Glass::clear(), Glass::lens()] {
        assert_eq!(glass.inner_shadow, None);
        let material = glass.inner_shadow(shadow);
        assert_eq!(material.inner_shadow, Some(shadow));
        let shader = terminal_shader(material.backdrop_effect(
            &light_colors(),
            3.0,
            GlassDynamics::default(),
        ));
        assert_eq!(
            &shader.uniforms()[GLASS_INNER_SHADOW_UNIFORM..GLASS_INNER_SHADOW_UNIFORM + 8],
            &shadow.uniforms().unwrap()
        );
    }
    for alpha in [0.0, 1.0] {
        assert!(
            Glass::lens()
                .inner_shadow(GlassShadow::new(
                    Color::BLACK.with_alpha(alpha),
                    0.0,
                    -2.0,
                    -3.0
                ))
                .inner_shadow
                .is_some()
        );
    }
    for index in 0..7 {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut values = shadow.uniforms().unwrap();
            values[index] = value;
            let invalid = GlassShadow {
                color: Color(values[0], values[1], values[2], values[3]),
                radius: values[4],
                offset_y: values[5],
                spread: values[6],
            };
            assert_eq!(Glass::lens().inner_shadow(invalid).inner_shadow, None);
            let material = Glass {
                inner_shadow: Some(invalid),
                ..Glass::lens()
            };
            let shader = terminal_shader(material.backdrop_effect(
                &light_colors(),
                3.0,
                GlassDynamics::default(),
            ));
            assert_eq!(
                shader
                    .uniforms()
                    .get(GLASS_INNER_SHADOW_UNIFORM + 7)
                    .copied()
                    .unwrap_or(0.0),
                0.0
            );
        }
    }
    for invalid in [
        GlassShadow {
            radius: -1.0,
            ..shadow
        },
        GlassShadow {
            color: Color(0.0, 0.0, 0.0, -0.1),
            ..shadow
        },
        GlassShadow {
            color: Color(0.0, 0.0, 0.0, 1.1),
            ..shadow
        },
    ] {
        assert_eq!(Glass::lens().inner_shadow(invalid).inner_shadow, None);
    }
}

#[test]
fn adaptive_tone_resolves_foreground_and_declares_the_shared_mean() {
    use cranpose_ui_graphics::SubstrateSpec;
    for foreground in [Color::BLACK, Color::WHITE, Color(f32::NAN, 0.0, 0.0, 1.0)] {
        let material = Glass::clear().adaptive_tone(foreground);
        assert!(material.adaptive_tone);
        let resolved = material.resolve(&light_colors());
        assert!(resolved.foreground_luma.is_finite());
        let shader = terminal_shader(resolved.backdrop_effect(3.0, GlassDynamics::default()));
        assert_eq!(shader.uniforms()[GLASS_ADAPTIVE_TONE_UNIFORM], 1.0);
        assert_eq!(shader.substrates(), &[SubstrateSpec::Mean]);
        let expected = if foreground.r().is_finite() {
            foreground.r()
        } else {
            0.2126 * light_colors().label.r()
                + 0.7152 * light_colors().label.g()
                + 0.0722 * light_colors().label.b()
        };
        assert!((resolved.foreground_luma - expected).abs() < 0.0001);
    }
    assert!(!Glass::clear().adaptive_tone);
    assert!(!Glass::regular().adaptive_tone);
}

#[test]
fn face_response_preserves_valid_ranges_and_rejects_invalid_values() {
    let response = GlassFaceResponse {
        gain: 0.97,
        start_dp: 1.0 / 3.0,
        end_dp: 2.0 / 3.0,
        illumination: 0.05,
    };
    for glass in [Glass::regular(), Glass::clear(), Glass::lens()] {
        assert!(glass.face_response.is_none());
        let material = glass.face_response(response);
        assert_eq!(material.face_response, Some(response));
        let shader = terminal_shader(material.backdrop_effect(
            &light_colors(),
            3.0,
            GlassDynamics::default(),
        ));
        assert_eq!(
            &shader.uniforms()[GLASS_FACE_RESPONSE_UNIFORM..GLASS_FACE_RESPONSE_UNIFORM + 4],
            &response.uniforms().unwrap()
        );
    }
    for gain in [0.0, 1.0] {
        assert!(
            GlassFaceResponse {
                gain,
                start_dp: 0.0,
                end_dp: 0.0,
                ..response
            }
            .uniforms()
            .is_some()
        );
    }
    let mut invalid = vec![
        GlassFaceResponse {
            gain: -0.1,
            ..response
        },
        GlassFaceResponse {
            gain: 1.1,
            ..response
        },
        GlassFaceResponse {
            start_dp: -1.0,
            ..response
        },
        GlassFaceResponse {
            end_dp: 0.0,
            ..response
        },
    ];
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        invalid.extend([
            GlassFaceResponse {
                gain: value,
                ..response
            },
            GlassFaceResponse {
                start_dp: value,
                ..response
            },
            GlassFaceResponse {
                end_dp: value,
                ..response
            },
            GlassFaceResponse {
                illumination: value,
                ..response
            },
        ]);
    }
    for response in invalid {
        assert!(response.uniforms().is_none());
        let shader = terminal_shader(Glass::clear().face_response(response).backdrop_effect(
            &light_colors(),
            3.0,
            GlassDynamics::default(),
        ));
        assert!(
            shader
                .uniforms()
                .get(GLASS_FACE_RESPONSE_UNIFORM..GLASS_FACE_RESPONSE_UNIFORM + 4)
                .is_none_or(|values| values.iter().all(|value| *value == 0.0))
        );
    }
}

#[test]
fn face_lighting_is_explicit_and_preserves_each_material_default() {
    for glass in [Glass::regular(), Glass::clear(), Glass::lens()] {
        assert!(glass.face_lighting);
        for enabled in [false, true] {
            let material = glass.clone().face_lighting(enabled);
            assert_eq!(material.face_lighting, enabled);
            let resolved = material.resolve(&light_colors());
            assert_eq!(resolved.face_lighting, enabled);
            let shader = terminal_shader(resolved.backdrop_effect(3.0, GlassDynamics::default()));
            assert_eq!(
                shader.uniforms()[GLASS_FACE_LIGHTING_OFF_UNIFORM],
                f32::from(!enabled)
            );
        }
    }
}

#[test]
fn layer_clipping_has_one_owner_for_material_coverage() {
    for clipped in [false, true] {
        let mut glass = Glass::regular();
        if !clipped {
            glass = glass.no_clip();
        }
        let shader =
            terminal_shader(glass.backdrop_effect(&light_colors(), 3.0, GlassDynamics::default()));
        assert_eq!(
            shader.uniforms()[GLASS_LAYER_CLIPPED_UNIFORM],
            f32::from(clipped)
        );
    }
}

#[test]
fn touch_radius_preserves_valid_values_and_normalizes_invalid_values() {
    for radius in [
        None,
        Some(-1.0),
        Some(0.0),
        Some(f32::NAN),
        Some(f32::INFINITY),
        Some(f32::NEG_INFINITY),
        Some(0.5),
        Some(144.0),
    ] {
        let expected = radius
            .filter(|value| value.is_finite() && *value > 0.0)
            .unwrap_or(58.0);
        for density in [1.0, 3.0] {
            let shader = terminal_shader(Glass::regular().backdrop_effect(
                &light_colors(),
                density,
                GlassDynamics {
                    touch_radius_dp: radius,
                    ..Default::default()
                },
            ));
            assert_eq!(shader.uniforms()[GLASS_TOUCH_RADIUS_UNIFORM], expected);
        }
    }
}

#[test]
fn material_instances_share_source_without_sharing_uniforms_or_overrides() {
    let resolved = Glass::regular().resolve(&light_colors());
    let mut first = terminal_shader(resolved.backdrop_effect(2.0, GlassDynamics::default()));
    let untouched = terminal_shader(resolved.backdrop_effect(1.0, GlassDynamics::default()));
    let unused_slot = RuntimeShader::MAX_USER_UNIFORMS - 1;
    first.set_float(unused_slot, 17.0);
    first.set_override("INSTANCE_ONLY", 1.0);
    let second = terminal_shader(resolved.backdrop_effect(1.0, GlassDynamics::default()));
    assert_eq!(second.uniforms(), untouched.uniforms());
    assert_eq!(second.overrides(), untouched.overrides());
    assert_eq!(
        second.uniforms().get(unused_slot).copied().unwrap_or(0.0),
        0.0
    );
    assert!(
        !second
            .overrides()
            .iter()
            .any(|(name, _)| *name == "INSTANCE_ONLY")
    );
    assert_eq!(first.uniforms()[unused_slot], 17.0);
    assert_eq!(first.uniforms()[GLASS_EFFECT_DENSITY_UNIFORM], 2.0);
    assert_eq!(second.uniforms()[GLASS_EFFECT_DENSITY_UNIFORM], 1.0);
    assert_eq!(first.source().as_ptr(), second.source().as_ptr());
}

#[test]
fn resolved_material_packs_wcksrd_and_dynamic_tint() {
    let resolved = Glass::lens()
        .refraction_depth(0.72)
        .refraction_depth_dp(21.0)
        .refraction_curve(0.8)
        .dispersion(0.42)
        .transmission_refraction(0.35)
        .meniscus_absorption(0.3)
        .blur_radius(3.0)
        .tint(Color::BLACK.with_alpha(0.8))
        .resolve(&light_colors());
    let effect = resolved.backdrop_effect(
        2.0,
        GlassDynamics {
            highlight_boost: 0.2,
            saturation_boost: 0.35,
            tint_alpha_multiplier: Some(0.25),
            ..Default::default()
        },
    );
    let RenderEffect::Chain { first, .. } = &effect else {
        panic!("macroscopic frost must precede the wcKSRD optical pass");
    };
    assert!(matches!(
        first.as_ref(),
        RenderEffect::Blur {
            radius_x: 6.0,
            radius_y: 6.0,
            ..
        }
    ));
    let shader = terminal_shader(effect);
    assert_eq!(shader.uniforms()[9], 0.72);
    assert_eq!(
        shader.uniforms()[GLASS_PHYSICAL_REFRACTION_DEPTH_UNIFORM],
        21.0
    );
    assert_eq!(
        shader.uniforms()[GLASS_PHYSICAL_REFRACTION_DEPTH_ENABLED_UNIFORM],
        1.0
    );
    assert_eq!(shader.uniforms()[GLASS_REFRACTION_CURVE_UNIFORM], 0.8);
    assert_eq!(shader.uniforms()[GLASS_DISPERSION_UNIFORM], 0.42);
    assert_eq!(
        shader.uniforms()[GLASS_TRANSMISSION_REFRACTION_UNIFORM],
        0.35
    );
    assert_eq!(shader.uniforms()[GLASS_MENISCUS_ABSORPTION_UNIFORM], 0.3);
    assert_eq!(shader.uniforms()[11], resolved.highlight + 0.2);
    assert_eq!(shader.uniforms()[18], resolved.saturation + 0.35);
    assert!((shader.uniforms()[17] - 0.2).abs() < 1.0e-6);
    assert_eq!(shader.uniforms()[GLASS_BLUR_RADIUS_UNIFORM], 0.0);
    assert_eq!(shader.uniforms()[GLASS_EFFECT_DENSITY_UNIFORM], 2.0);
}

#[test]
fn heavy_backdrop_blur_rides_the_gaussian_pass_alone() {
    let effect = Glass::regular()
        .resolve(&light_colors())
        .backdrop_effect(3.0, GlassDynamics::default());
    let RenderEffect::Chain { first, .. } = &effect else {
        panic!("heavy blur must ride the separable Gaussian pre-pass");
    };
    assert!(matches!(
        first.as_ref(),
        RenderEffect::Blur {
            radius_x: 24.0,
            radius_y: 24.0,
            ..
        }
    ));
    assert_eq!(
        terminal_shader(effect).uniforms()[GLASS_BLUR_RADIUS_UNIFORM],
        0.0
    );
}

#[test]
fn featherweight_backdrop_blur_stays_in_the_optical_tap() {
    let RenderEffect::Shader { shader } = Glass::regular()
        .blur_radius(0.5)
        .resolve(&light_colors())
        .backdrop_effect(3.0, GlassDynamics::default())
    else {
        panic!("a sub-cap blur must not build a Gaussian chain");
    };
    assert_eq!(shader.uniforms()[GLASS_BLUR_RADIUS_UNIFORM], 1.5);
}

#[test]
fn raised_tint_density_stays_premultiplied_alpha_safe() {
    let effect = Glass::lens()
        .tint(Color::WHITE.with_alpha(0.8))
        .resolve(&light_colors())
        .backdrop_effect(
            1.0,
            GlassDynamics {
                tint_alpha_multiplier: Some(2.0),
                ..Default::default()
            },
        );
    assert_eq!(terminal_shader(effect).uniforms()[17], 1.0);
}

#[test]
fn optical_activity_reaches_identity_without_removing_the_glass_geometry() {
    let resolved = Glass::lens()
        .refraction_depth(0.72)
        .refraction_curve(0.8)
        .dispersion(0.42)
        .blur_radius(3.0)
        .saturation(1.4)
        .highlight(0.6)
        .lift(0.2)
        .tint(Color::BLACK.with_alpha(0.8))
        .no_clip()
        .resolve(&light_colors());
    let morph = GlassMorph {
        node_size: (120.0, 72.0),
        primary: (60.0, 36.0, 96.0, 52.0, -1.0),
        wobble_amplitude: 3.0,
        bulge_amplitude: 4.0,
        deformation: Some(GlassDeformation::incompressible((1.0, 0.0), 1.25)),
        ..Default::default()
    };

    let RenderEffect::Shader { shader } = resolved.backdrop_effect(
        2.0,
        GlassDynamics {
            activity: Some(0.0),
            morph: Some(morph),
            ..Default::default()
        },
    ) else {
        panic!("material must resolve to the shared wcKSRD shader");
    };
    let uniforms = shader.uniforms();
    assert_eq!(uniforms[GLASS_ACTIVITY_UNIFORM], 0.0);
    assert_eq!(uniforms[9], 0.0);
    assert_eq!(uniforms[GLASS_REFRACTION_CURVE_UNIFORM], 0.0);
    assert_eq!(uniforms[GLASS_DISPERSION_UNIFORM], 0.0);
    assert_eq!(uniforms[GLASS_TRANSMISSION_REFRACTION_UNIFORM], 0.0);
    assert_eq!(uniforms[11], 0.0);
    assert_eq!(uniforms[17], 0.0);
    assert_eq!(uniforms[18], 1.0);
    assert_eq!(uniforms[20], 0.0);
    assert_eq!(uniforms[21], 0.0);
    assert_eq!(uniforms[24], 1.0);
    assert_eq!(uniforms[28], 0.0);
    assert_eq!(uniforms[GLASS_BLUR_RADIUS_UNIFORM], 0.0);
    assert_eq!(uniforms[91], 0.0);
    assert_eq!(uniforms[102], 0.0);
    assert_eq!(
        &uniforms[GLASS_RESTING_TINT_UNIFORM..GLASS_RESTING_TINT_UNIFORM + 4],
        &[0.0, 0.0, 0.0, 0.0]
    );
    assert_eq!(uniforms[32], 0.0);
    assert_eq!(uniforms[26], 0.0);
    assert_eq!(&uniforms[108..110], &[1.0, 1.0]);
    assert_eq!(&uniforms[2..6], &[60.0, 36.0, 96.0, 52.0]);
}

#[test]
fn surface_refraction_is_explicit_and_reaches_the_shader() {
    for glass in [Glass::regular(), Glass::lens()] {
        assert_eq!(glass.refraction, GlassRefraction::Radial);
        let surface = glass.surface_refraction(31.0);
        assert_eq!(
            surface.refraction,
            GlassRefraction::Surface { reach_dp: 31.0 }
        );
        let shader = terminal_shader(surface.backdrop_effect(
            &light_colors(),
            3.0,
            GlassDynamics::default(),
        ));
        assert_eq!(shader.uniforms()[GLASS_REFRACTION_MODE_UNIFORM], 1.0);
        assert_eq!(shader.uniforms()[GLASS_EDGE_REFRACTION_REACH_UNIFORM], 31.0);
        assert!(
            !shader
                .overrides()
                .iter()
                .any(|(name, value)| *name == "GLASS_DIRECTIONAL_REFRACTION_OFF" && *value != 0.0)
        );
    }
}

#[test]
fn edge_refraction_preserves_face_zoom_and_scales_outward_reach_with_contact() {
    assert_eq!(GlassRefraction::default(), GlassRefraction::Radial);
    let glass = Glass::lens()
        .surface_refraction(31.0)
        .edge_refraction(12.0)
        .optical_zoom(1.2);
    assert_eq!(
        glass.refraction,
        GlassRefraction::EdgeLens { reach_dp: 12.0 }
    );
    for (activity, press, reach) in [(1.0, 1.0, 12.0), (0.5, 0.5, 3.0), (0.0, 1.0, 0.0)] {
        let shader = terminal_shader(glass.backdrop_effect(
            &light_colors(),
            3.0,
            GlassDynamics {
                activity: Some(activity),
                press_depth: Some(press),
                ..Default::default()
            },
        ));
        assert_eq!(shader.uniforms()[GLASS_REFRACTION_MODE_UNIFORM], 2.0);
        assert_eq!(
            shader.uniforms()[GLASS_EDGE_REFRACTION_REACH_UNIFORM],
            reach
        );
        assert_eq!(
            shader.uniforms()[GLASS_OPTICAL_ZOOM_UNIFORM],
            1.0 + 0.2 * activity
        );
    }
    assert_eq!(
        glass.surface_refraction(31.0).refraction,
        GlassRefraction::Surface { reach_dp: 31.0 }
    );
}

#[test]
fn edge_refraction_captures_the_farthest_spectral_ray() {
    for (reach, dispersion, required) in
        [(20.0, 1.0, 101.67), (0.0, 1.0, 42.78), (20.0, 0.0, 60.88)]
    {
        let shader = terminal_shader(
            Glass::lens()
                .edge_refraction(reach)
                .dispersion(dispersion)
                .backdrop_effect(&light_colors(), 3.0, GlassDynamics::default()),
        );
        assert!(shader.input_padding() >= required);
    }
}

#[test]
fn directional_refraction_rejects_invalid_reach_at_both_public_entry_points() {
    for input in [-1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, 12.0] {
        let expected = if input.is_finite() {
            input.max(0.0)
        } else {
            0.0
        };
        for (built, direct, normalized) in [
            (
                Glass::lens().edge_refraction(input),
                GlassRefraction::EdgeLens { reach_dp: input },
                GlassRefraction::EdgeLens { reach_dp: expected },
            ),
            (
                Glass::regular().surface_refraction(input),
                GlassRefraction::Surface { reach_dp: input },
                GlassRefraction::Surface { reach_dp: expected },
            ),
        ] {
            assert_eq!(built.refraction, normalized);
            let direct = Glass {
                refraction: direct,
                ..built
            };
            let shader = terminal_shader(direct.backdrop_effect(
                &light_colors(),
                1.0,
                GlassDynamics::default(),
            ));
            assert_eq!(
                shader.uniforms()[GLASS_EDGE_REFRACTION_REACH_UNIFORM],
                expected
            );
        }
    }
}

#[test]
fn surface_reach_is_independent_of_depth_and_tracks_contact() {
    for depth in [4.0, 15.5, 30.0] {
        for (activity, press, reach) in [(1.0, 1.0, 31.0), (0.5, 0.5, 7.75), (0.0, 1.0, 0.0)] {
            let shader = terminal_shader(
                Glass::regular()
                    .surface_refraction(31.0)
                    .refraction_depth_dp(depth)
                    .backdrop_effect(
                        &light_colors(),
                        3.0,
                        GlassDynamics {
                            activity: Some(activity),
                            press_depth: Some(press),
                            ..Default::default()
                        },
                    ),
            );
            assert_eq!(
                shader.uniforms()[GLASS_EDGE_REFRACTION_REACH_UNIFORM],
                reach
            );
            assert_eq!(
                shader.uniforms()[GLASS_PHYSICAL_REFRACTION_DEPTH_UNIFORM],
                depth * activity * press
            );
            assert!(shader.input_padding() >= 31.0);
        }
    }
}

#[test]
fn resting_surface_tint_survives_zero_optical_activity() {
    let tint = Color::BLACK.with_alpha(0.11);
    let RenderEffect::Shader { shader } = Glass::lens()
        .no_clip()
        .resolve(&light_colors())
        .backdrop_effect(
            1.0,
            GlassDynamics {
                activity: Some(0.0),
                resting_tint: Some(tint),
                ..Default::default()
            },
        )
    else {
        panic!("resting surface must use the shared wcKSRD shader");
    };
    assert_eq!(
        &shader.uniforms()[GLASS_RESTING_TINT_UNIFORM..GLASS_RESTING_TINT_UNIFORM + 4],
        &[tint.r(), tint.g(), tint.b(), tint.a()]
    );
    assert_eq!(shader.uniforms()[GLASS_ACTIVITY_UNIFORM], 0.0);
}

#[test]
fn full_optical_activity_preserves_the_resolved_material() {
    let resolved = Glass::lens()
        .refraction_depth(0.72)
        .refraction_curve(0.8)
        .dispersion(0.42)
        .blur_radius(3.0)
        .resolve(&light_colors());
    let shader = terminal_shader(resolved.backdrop_effect(
        2.0,
        GlassDynamics {
            activity: Some(1.0),
            ..Default::default()
        },
    ));
    let uniforms = shader.uniforms();
    assert_eq!(uniforms[GLASS_ACTIVITY_UNIFORM], 1.0);
    assert_eq!(uniforms[9], 0.72);
    assert_eq!(
        uniforms[GLASS_PHYSICAL_REFRACTION_DEPTH_ENABLED_UNIFORM],
        0.0
    );
    assert_eq!(uniforms[GLASS_REFRACTION_CURVE_UNIFORM], 0.8);
    assert_eq!(uniforms[GLASS_DISPERSION_UNIFORM], 0.42);
    assert_eq!(uniforms[GLASS_TRANSMISSION_REFRACTION_UNIFORM], 1.0);
    assert_eq!(uniforms[GLASS_BLUR_RADIUS_UNIFORM], 0.0);
}

#[test]
fn morph_geometry_and_incompressible_strain_are_packed() {
    let deformation = GlassDeformation::incompressible((0.0, 2.0), 1.25);
    let morph = GlassMorph {
        node_size: (78.0, 59.0),
        primary: (39.0, 29.5, 58.0, 39.0, -1.0),
        shapes: vec![(70.0, 29.5, 40.0, 40.0, -1.0)],
        glue: 8.0,
        wobble_amplitude: 1.0,
        wobble_phase: 0.5,
        bulge_amplitude: 2.0,
        bulge_direction: 0.25,
        ellipse_blend: 0.3,
        capsule_smoothing_dp: 4.0,
        deformation: Some(deformation),
        zoom_anchor: (0.0, 0.0),
    };
    let RenderEffect::Shader { shader } = Glass::lens()
        .no_clip()
        .resolve(&light_colors())
        .backdrop_effect(
            1.0,
            GlassDynamics {
                morph: Some(morph),
                ..Default::default()
            },
        )
    else {
        panic!("morph must use the shared wcKSRD shader");
    };
    let uniforms = shader.uniforms();
    assert_eq!(&uniforms[0..6], &[78.0, 59.0, 39.0, 29.5, 58.0, 39.0]);
    assert_eq!(uniforms[30], 1.0);
    assert_eq!(&uniforms[106..110], &[0.0, 1.0, 1.25, 0.8]);
    assert_eq!(uniforms[110], 0.3);
    assert_eq!(
        uniforms[cranpose_ui_graphics::liquid_glass::GLASS_CAPSULE_SMOOTHING_UNIFORM],
        4.0
    );
    assert!(shader.output_padding() > 0.0);
}

#[test]
fn capsule_smoothing_is_finite_nonnegative_and_invalidates_the_morph() {
    for value in [-1.0, 0.0, 4.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let morph = GlassMorph {
            node_size: (120.0, 80.0),
            primary: (60.0, 40.0, 111.0, 70.0, -1.0),
            capsule_smoothing_dp: value,
            ..Default::default()
        };
        let expected = if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        };
        let shader = terminal_shader(Glass::lens().backdrop_effect(
            &light_colors(),
            3.0,
            GlassDynamics {
                morph: Some(morph.clone()),
                ..Default::default()
            },
        ));
        assert_eq!(
            shader.uniforms()[cranpose_ui_graphics::GLASS_CAPSULE_SMOOTHING_UNIFORM],
            expected
        );
        assert!(glass_morphs_match(&morph, &morph));
        let changed = GlassMorph {
            capsule_smoothing_dp: 5.0,
            ..morph.clone()
        };
        assert!(!glass_morphs_match(&morph, &changed));
    }
}

#[test]
fn optical_builders_hold_their_own_legal_range() {
    let glass = Glass::regular()
        .fold_depth(-4.0)
        .optical_zoom(0.5)
        .rim_reflection(9.0);
    assert_eq!(glass.fold_depth, 0.0, "a fold cannot be negative deep");
    assert_eq!(glass.optical_zoom, 1.0, "a lens never shrinks its face");
    assert_eq!(
        glass.rim_reflection, 2.0,
        "the rim tops out at twice the line"
    );

    let glass = Glass::regular()
        .fold_depth(6.0)
        .optical_zoom(1.4)
        .rim_reflection(0.25);
    assert_eq!(glass.fold_depth, 6.0);
    assert_eq!(glass.optical_zoom, 1.4);
    assert_eq!(glass.rim_reflection, 0.25);
}

#[test]
fn ink_recolor_records_its_color_at_a_clamped_strength() {
    let red = Color::from_rgb_u8(255, 0, 0);
    let glass = Glass::regular().ink_recolor(red, 3.0);
    assert_eq!(glass.ink_recolor, Some((red, 1.0)));

    let glass = Glass::regular().ink_recolor(red, -1.0);
    assert_eq!(glass.ink_recolor, Some((red, 0.0)));
    assert_eq!(Glass::regular().ink_recolor, None, "no recolor by default");
}

#[test]
fn a_touched_up_surface_amplifies_what_it_already_is() {
    let dynamics = GlassDynamics::default().touched_up(1.0, None, (10.0, 4.0));
    assert!(
        dynamics.highlight_boost > 0.0,
        "a pressed surface goes brighter"
    );
    assert!(dynamics.saturation_boost > 0.0, "and more saturated");
    assert_eq!(
        dynamics.touch,
        Some((10.0, 4.0, 1.0)),
        "with no finger the glow sits at the surface's own heart"
    );
    assert_eq!(
        dynamics.resting_tint, None,
        "a touch never repaints the surface with another color"
    );
}

#[test]
fn a_touch_glow_follows_the_finger_and_accumulates_to_full() {
    let dynamics = GlassDynamics::default()
        .touched_up(0.5, Some((3.0, 7.0)), (10.0, 4.0))
        .touched_up(0.8, Some((3.0, 7.0)), (10.0, 4.0));
    let (x, y, intensity) = dynamics.touch.expect("a pressed surface glows");
    assert_eq!(
        (x, y),
        (3.0, 7.0),
        "the glow tracks the finger, not the center"
    );
    assert_eq!(intensity, 1.0, "carried press saturates at full");
}

#[test]
fn an_unpressed_surface_is_left_exactly_as_it_was() {
    let resting = GlassDynamics::default();
    let untouched = resting
        .clone()
        .touched_up(0.0, Some((3.0, 7.0)), (10.0, 4.0));
    assert_eq!(untouched.highlight_boost, resting.highlight_boost);
    assert_eq!(untouched.saturation_boost, resting.saturation_boost);
    assert_eq!(untouched.touch, None, "no press, no glow");
}

#[test]
fn an_off_axis_strain_widens_the_support_by_the_affine_rows_times_the_half_extents() {
    let resolved = Glass::regular()
        .shape(LiquidShape::RoundedRect(0.0))
        .blur_radius(0.0)
        .shadow(false)
        .no_clip()
        .resolve(&light_colors());
    let angle = 22.5f32.to_radians();
    let morph = GlassMorph {
        node_size: (600.0, 600.0),
        primary: (300.0, 300.0, 200.0, 200.0, 0.0),
        deformation: Some(GlassDeformation::incompressible(
            (angle.cos(), angle.sin()),
            2.0,
        )),
        ..Default::default()
    };
    let RenderEffect::Shader { shader } = resolved.backdrop_effect(
        1.0,
        GlassDynamics {
            activity: Some(1.0),
            morph: Some(morph),
            ..Default::default()
        },
    ) else {
        panic!("an unblurred glass is one shader");
    };
    let support = shader
        .output_support()
        .expect("a morphing glass declares its support");
    let half_x = 100.0 * (2.0 * angle.cos().powi(2) + 0.5 * angle.sin().powi(2))
        + 100.0 * (1.5 * angle.sin() * angle.cos());
    assert!(
        (half_x - 231.066).abs() < 0.01,
        "the example's half extent is {half_x}"
    );
    assert!(
        support.x <= 300.0 - half_x && support.x + support.width >= 300.0 + half_x,
        "the support {support:?} must hold the strained square's x extent {half_x}"
    );
    let half_y = 100.0 * (1.5 * angle.sin() * angle.cos())
        + 100.0 * (2.0 * angle.sin().powi(2) + 0.5 * angle.cos().powi(2));
    assert!(
        (half_y - 125.0).abs() < 0.01,
        "the example's y half extent is {half_y}"
    );
    assert!(
        support.y <= 300.0 - half_y && support.y + support.height >= 300.0 + half_y,
        "the support {support:?} must hold the strained square's y extent {half_y}"
    );
}

#[test]
fn a_morphing_glass_declares_its_output_support_around_its_shapes_and_their_reach() {
    let resolved = Glass::regular()
        .shape(LiquidShape::RoundedRect(12.0))
        .blur_radius(0.0)
        .shadow(true)
        .no_clip()
        .resolve(&light_colors());
    let morph = GlassMorph {
        node_size: (280.0, 160.0),
        primary: (140.0, 80.0, 120.0, 40.0, -1.0),
        shapes: vec![(230.0, 80.0, 40.0, 40.0, -1.0)],
        glue: 20.0,
        wobble_amplitude: 3.0,
        bulge_amplitude: 8.0,
        deformation: Some(GlassDeformation::incompressible((1.0, 0.0), 1.25)),
        ..Default::default()
    };
    let dynamics = GlassDynamics {
        activity: Some(1.0),
        morph: Some(morph),
        ..Default::default()
    };
    let RenderEffect::Shader { shader } = resolved.backdrop_effect(2.0, dynamics) else {
        panic!("an unblurred glass is one shader");
    };
    let support = shader
        .output_support()
        .expect("a morphing glass declares its support");
    let shape_reach = (230.0 + 20.0) - (140.0 + 60.0);
    let shadow_reach =
        resolved.shadow_radius + resolved.shadow_offset_y.abs() + resolved.shadow_spread.max(0.0);
    let field_reach = 3.0 * 2.0 + 8.0 + shape_reach + 40.0 + shadow_reach + 4.0;
    let smallest_strain = 0.8;
    let reach = field_reach / smallest_strain;
    let contains = |x: f32, y: f32| {
        support.x <= x
            && support.y <= y
            && support.x + support.width >= x
            && support.y + support.height >= y
    };
    for (cx, cy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let corner = ((60.0 + reach) * cx, (20.0 + reach) * cy);
        let stretched = (140.0 + corner.0 * 1.25, 80.0 + corner.1 * 0.8);
        assert!(
            contains(stretched.0, stretched.1 - resolved.shadow_offset_y.abs())
                && contains(stretched.0, stretched.1 + resolved.shadow_offset_y.abs()),
            "{support:?} must hold the strained, dilated corner {stretched:?}"
        );
    }
    assert!(
        contains(230.0 + 20.0 + reach, 80.0),
        "the glued shape's reach is inside"
    );
    assert!(shader.output_padding() >= field_reach);

    let RenderEffect::Shader { shader } = resolved.backdrop_effect(2.0, GlassDynamics::default())
    else {
        panic!("an unblurred glass is one shader");
    };
    assert_eq!(
        shader.output_support(),
        None,
        "cover-mode glass fills its rect"
    );
}
