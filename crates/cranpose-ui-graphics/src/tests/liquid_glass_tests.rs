use super::*;

#[test]
fn a_content_mask_declares_it_preserves_transparency() {
    let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);
    let RenderEffect::Shader { shader: backdrop } = liquid_glass_runtime_effect(shader.clone())
    else {
        panic!("a glass without an edge lens is one shader");
    };
    assert!(!backdrop.preserves_transparency());
    shader.set_float(112, 1.0);
    let RenderEffect::Shader { shader: mask } = liquid_glass_runtime_effect(shader) else {
        panic!("a content mask is one shader");
    };
    assert!(mask.preserves_transparency());
}

fn rect() -> LiquidGlassRect {
    LiquidGlassRect {
        left: 100.0,
        top: 50.0,
        width: 200.0,
        height: 100.0,
        tint_color: Color(0.5, 0.5, 1.0, 0.1),
    }
}

fn shader_lines_reading(slot: usize) -> Vec<&'static str> {
    let needles = [format!("get_float({slot}u)"), format!("get_vec2({slot}u)")];
    LIQUID_GLASS_WGSL
        .lines()
        .filter(|line| needles.iter().any(|needle| line.contains(needle)))
        .collect()
}

#[test]
fn projected_optics_disable_rim_scissors_and_restore_them_at_identity() {
    let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);
    shader.set_float2(GLASS_OPTICAL_PROJECTION_UNIFORM, 1.2, 0.75);
    specialize_liquid_glass_with_folds(&mut shader, true);
    assert_eq!(shader.draw_split(), None);
    shader.set_float2(GLASS_OPTICAL_PROJECTION_UNIFORM, 1.0, 1.0);
    specialize_liquid_glass_with_folds(&mut shader, true);
    assert_eq!(shader.draw_split(), Some(GLASS_RIM_DRAW_OVERRIDE));
}

#[test]
fn every_specialization_flag_is_a_shader_override_and_guards_all_of_its_slot_reads() {
    for specialization in LIQUID_GLASS_SPECIALIZATIONS {
        let declaration = format!("override {}: bool = false;", specialization.flag);
        assert!(
            LIQUID_GLASS_WGSL.contains(&declaration),
            "`{declaration}` missing from liquid_glass.wgsl"
        );
        let mut guarded_reads = 0;
        for slot in specialization.slots {
            for line in shader_lines_reading(*slot) {
                assert!(
                    line.contains(specialization.flag),
                    "slot {slot} is read outside its `{}` guard: `{}`",
                    specialization.flag,
                    line.trim()
                );
                guarded_reads += 1;
            }
        }
        assert!(
            guarded_reads > 0 || specialization.slots.is_empty(),
            "`{}` guards no uniform read; the flag would fold nothing",
            specialization.flag
        );
    }
    let declared: Vec<&str> = LIQUID_GLASS_WGSL
        .lines()
        .filter_map(|line| line.strip_prefix("override "))
        .filter(|rest| rest.contains(": bool"))
        .filter_map(|rest| rest.split(':').next())
        .collect();
    for flag in &declared {
        assert!(
            LIQUID_GLASS_SPECIALIZATIONS
                .iter()
                .any(|specialization| specialization.flag == *flag),
            "shader override `{flag}` is missing from LIQUID_GLASS_SPECIALIZATIONS, so \
             nothing ever raises it"
        );
    }
    assert_eq!(declared.len(), LIQUID_GLASS_SPECIALIZATIONS.len());
}

/// The flags a material's uniforms fold, whatever the platform folds.
fn raised_flags(effect: &RenderEffect) -> Vec<&'static str> {
    let RenderEffect::Shader { shader } = effect else {
        panic!("liquid glass must be one runtime shader");
    };
    let mut shader = (**shader).clone();
    specialize_liquid_glass_with_folds(&mut shader, true);
    shader.overrides().iter().map(|(flag, _)| *flag).collect()
}

#[test]
fn glass_substrates_refresh_for_frost_tone_and_pane_combinations() {
    for folds in [false, true] {
        let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);
        for radius in [0.0, 8.0, 24.0, 0.0] {
            for frost in [false, true] {
                for tone in [false, true] {
                    shader.set_float(GLASS_PANE_BLEND_UNIFORM, radius);
                    shader.set_float(GLASS_ADAPTIVE_FROST_UNIFORM, f32::from(frost));
                    shader.set_float(GLASS_ADAPTIVE_TONE_UNIFORM, f32::from(tone));
                    specialize_liquid_glass_with_folds(&mut shader, folds);
                    let mut expected = Vec::new();
                    if frost {
                        expected.push(SubstrateSpec::Blur {
                            radius_px: GLASS_ADAPTIVE_NEIGHBOURHOOD_DP,
                        });
                    }
                    if tone {
                        expected.push(SubstrateSpec::Mean);
                    }
                    if radius > 0.0 {
                        expected.push(SubstrateSpec::Blur { radius_px: radius });
                    }
                    assert_eq!(shader.substrates(), expected);
                }
            }
        }
    }
}

#[test]
fn a_resting_glass_keeps_its_substrate_declaration() {
    let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);
    shader.set_float(GLASS_ADAPTIVE_FROST_UNIFORM, 0.42);
    shader.set_float(GLASS_EFFECT_DENSITY_UNIFORM, 2.0);
    shader.set_float(GLASS_ACTIVITY_UNIFORM, 0.0);
    specialize_liquid_glass_with_folds(&mut shader, true);
    assert_eq!(
        shader.substrates().len(),
        1,
        "the declaration sets the capture geometry, which must not follow activity"
    );
}

#[test]
fn edge_lens_stages_preserve_intermediate_images_and_final_support() {
    fn shaders(effect: &RenderEffect) -> Vec<&RuntimeShader> {
        match effect {
            RenderEffect::Shader { shader } => vec![shader],
            RenderEffect::Chain { first, second } => {
                let mut passes = shaders(first);
                passes.extend(shaders(second));
                passes
            }
            _ => panic!("glass must contain shader stages"),
        }
    }
    let support = crate::Rect {
        x: -2.0,
        y: -3.0,
        width: 120.0,
        height: 80.0,
    };
    for activity in [0.0, 0.5, 1.0] {
        let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);
        shader.set_float(GLASS_REFRACTION_MODE_UNIFORM, 2.0);
        shader.set_float(GLASS_ACTIVITY_UNIFORM, activity);
        shader.set_input_padding(30.0);
        shader.set_output_padding(8.0);
        shader.set_output_support(Some(support));
        let effect = liquid_glass_runtime_effect(shader.clone());
        let passes = shaders(&effect);
        assert_eq!(passes.len(), 3);
        for (index, pass) in passes.iter().enumerate() {
            assert_eq!(
                pass.uniforms()[GLASS_OPTICAL_STAGE_UNIFORM],
                (index + 1) as f32
            );
            assert_eq!(pass.uniforms()[GLASS_ACTIVITY_UNIFORM], activity);
            assert!(pass.batched_source());
            assert_eq!(pass.input_padding(), 30.0);
            if index < 2 {
                assert_eq!(pass.output_support(), None);
                assert_eq!(pass.output_padding(), 0.0);
                assert_eq!(pass.draw_split(), None);
            } else {
                assert_eq!(pass.output_support(), Some(support));
                assert_eq!(pass.output_padding(), 8.0);
            }
        }
        for (mode, mask, stage) in [
            (0.0, 0.0, 0.0),
            (1.0, 0.0, 0.0),
            (2.0, 1.0, 0.0),
            (2.0, 0.0, 1.0),
            (2.0, 0.0, 2.0),
            (2.0, 0.0, 3.0),
        ] {
            let mut single = shader.clone();
            single.set_float(GLASS_REFRACTION_MODE_UNIFORM, mode);
            single.set_float(112, mask);
            single.set_float(GLASS_OPTICAL_STAGE_UNIFORM, stage);
            let effect = liquid_glass_runtime_effect(single);
            let passes = shaders(&effect);
            assert_eq!(passes.len(), 1);
            assert_eq!(passes[0].uniforms()[GLASS_OPTICAL_STAGE_UNIFORM], stage);
        }
    }
}

#[test]
fn cached_glass_specialization_tracks_flags_and_substrate_radius() {
    let mut template = RuntimeShader::new(LIQUID_GLASS_WGSL);
    template.set_override("CALLER", 7.0);
    specialize_liquid_glass_with_folds(&mut template, true);
    for (activity, frost, density, rim) in [
        (1.0, 0.5, 1.0, 0.0),
        (1.0, 0.5, 2.0, 0.0),
        (0.0, 0.5, 2.0, 1.0),
        (0.5, 0.0, 3.0, 0.0),
        (1.0, 0.5, 1.0, 0.0),
    ] {
        for _ in 0..2 {
            let mut shader = template.clone();
            shader.set_float(GLASS_ACTIVITY_UNIFORM, activity);
            shader.set_float(GLASS_ADAPTIVE_FROST_UNIFORM, frost);
            shader.set_float(GLASS_EFFECT_DENSITY_UNIFORM, density);
            shader.set_float(GLASS_RIM_STYLE_UNIFORM, rim);
            specialize_liquid_glass_with_folds(&mut shader, true);
            for (flag, raised) in [
                ("GLASS_ADAPTIVE_FROST_OFF", frost <= 0.0),
                ("GLASS_RIM_STYLE_OFF", rim <= 0.0),
            ] {
                assert_eq!(shader.overrides().contains(&(flag, 1.0)), raised, "{flag}");
            }
            let substrate = (frost > 0.0).then_some(SubstrateSpec::Blur {
                radius_px: GLASS_ADAPTIVE_NEIGHBOURHOOD_DP * density,
            });
            assert_eq!(shader.substrates(), substrate.as_slice());
            assert_eq!(shader.draw_split(), Some(GLASS_RIM_DRAW_OVERRIDE));
            assert!(shader.overrides().contains(&("CALLER", 7.0)));
            assert_eq!(shader.uniforms()[GLASS_EFFECT_DENSITY_UNIFORM], density);
        }
    }
    assert!(template.substrates().is_empty());
    assert!(
        template
            .overrides()
            .contains(&("GLASS_ADAPTIVE_FROST_OFF", 1.0))
    );
}

/// A material that animates must not change which pipeline draws it.
///
/// A specialization folding on a value an animation *ends* on gives the
/// end of every press its own `override` set, and a set nothing has
/// compiled is a backend shader compile inside the frame that reaches
/// it. `GLASS_FULL_ACTIVITY` and `GLASS_FULL_TRANSMISSION` were exactly
/// that: touching a liquid tab built four pipelines and took half a
/// second on a backend with no pipeline cache to fall back on. A fold
/// that saves ALU only in the frame a person is waiting in is not worth
/// having, so these two values carry no fold at all.
#[test]
fn animating_a_material_end_to_end_asks_for_one_pipeline() {
    let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);
    let mut sets: Vec<Vec<(&'static str, f64)>> = Vec::new();
    for step in 0..=40u16 {
        let value = f32::from(step) / 40.0;
        shader.set_float(GLASS_ACTIVITY_UNIFORM, value);
        shader.set_float(GLASS_TRANSMISSION_REFRACTION_UNIFORM, value);
        specialize_liquid_glass_with_folds(&mut shader, true);
        let set = shader.overrides().to_vec();
        if !sets.contains(&set) {
            sets.push(set);
        }
    }
    assert_eq!(
        sets.len(),
        1,
        "one press walks through {} override sets, and every one of them is a pipeline \
         the backend compiles inside the frame that first needs it: {sets:?}",
        sets.len()
    );
}

#[test]
fn a_plain_pane_raises_every_flag() {
    let flags = raised_flags(&liquid_glass_effect(
        &rect(),
        &LiquidGlassSpec::default(),
        800.0,
        600.0,
    ));
    let every_flag: Vec<&str> = LIQUID_GLASS_SPECIALIZATIONS
        .iter()
        .map(|specialization| specialization.flag)
        .collect();
    let mut sorted = every_flag.clone();
    sorted.sort_unstable();
    assert_eq!(
        flags, sorted,
        "a plain pane uses no optional feature, so every flag folds"
    );
}

#[test]
fn respecializing_mutated_uniforms_matches_fresh_shader_and_preserves_caller_override() {
    let mut shader = RuntimeShader::new(LIQUID_GLASS_WGSL);
    shader.set_override("CALLER_OVERRIDE", 7.0);
    specialize_liquid_glass_with_folds(&mut shader, true);
    shader.set_float(GLASS_RIM_STYLE_UNIFORM, 1.0);
    specialize_liquid_glass_with_folds(&mut shader, true);

    let mut fresh = RuntimeShader::new(LIQUID_GLASS_WGSL);
    fresh.set_override("CALLER_OVERRIDE", 7.0);
    fresh.set_float(GLASS_RIM_STYLE_UNIFORM, 1.0);
    specialize_liquid_glass_with_folds(&mut fresh, true);
    assert_eq!(shader.overrides(), fresh.overrides());
    assert_eq!(shader.overrides_hash(), fresh.overrides_hash());
    assert!(shader.overrides().contains(&("CALLER_OVERRIDE", 7.0)));

    shader.set_float(GLASS_RIM_STYLE_UNIFORM, 0.0);
    specialize_liquid_glass_with_folds(&mut shader, true);
    let mut inactive = RuntimeShader::new(LIQUID_GLASS_WGSL);
    inactive.set_override("CALLER_OVERRIDE", 7.0);
    specialize_liquid_glass_with_folds(&mut inactive, true);
    assert_eq!(shader.overrides(), inactive.overrides());
    assert_eq!(shader.overrides_hash(), inactive.overrides_hash());
    assert!(shader.overrides().contains(&("CALLER_OVERRIDE", 7.0)));
}

#[test]
fn a_blurred_dispersive_loupe_keeps_its_features_live() {
    let flags = raised_flags(&liquid_loupe_effect(
        (200.0, 120.0),
        &LiquidLoupeSpec::default(),
    ));
    for live in ["GLASS_LOUPE_OFF", "GLASS_DISPERSION_OFF"] {
        assert!(
            !flags.contains(&live),
            "{live} must stay live for a loupe: {flags:?}"
        );
    }
    assert!(flags.contains(&"GLASS_FOLD_OFF"));
    assert!(flags.contains(&"GLASS_SCENE_SHAPES_OFF"));

    let blurred = liquid_glass_effect(
        &rect(),
        &LiquidGlassSpec {
            blur_radius: 1.5,
            ..LiquidGlassSpec::default()
        },
        800.0,
        600.0,
    );
    assert!(
        !raised_flags(&blurred).contains(&"GLASS_OPTICAL_BLUR_OFF"),
        "an optical blur radius keeps the wcKSRD footprint live"
    );
}

#[test]
fn liquid_glass_spec_defaults_match_wcksrd() {
    let spec = LiquidGlassSpec::default();
    assert_eq!(spec.corner_radius, 28.0);
    assert_eq!(spec.refraction_depth, 0.34);
    assert_eq!(spec.refraction_curve, 0.25);
    assert_eq!(spec.blur_radius, 0.0);
    assert_eq!(spec.saturation, 1.0);
    assert_eq!(spec.lift, 0.0);
    assert_eq!(spec.contrast, 1.0);
    assert_eq!(spec.meniscus_absorption, 1.0);
}

#[test]
fn liquid_glass_effect_packs_the_single_optical_program() {
    let spec = LiquidGlassSpec {
        refraction_depth: 0.72,
        refraction_curve: 0.8,
        blur_radius: 6.0,
        saturation: 1.6,
        lift: 0.12,
        contrast: 1.05,
        meniscus_absorption: 0.3,
        dither: 1.0,
        ..LiquidGlassSpec::default()
    };
    let RenderEffect::Shader { shader } = liquid_glass_effect(&rect(), &spec, 800.0, 600.0) else {
        panic!("liquid glass must be one runtime shader");
    };
    let uniforms = shader.uniforms();
    assert_eq!(&uniforms[0..6], &[800.0, 600.0, 200.0, 100.0, 200.0, 100.0]);
    assert_eq!(uniforms[9], 0.72);
    assert_eq!(uniforms[GLASS_REFRACTION_CURVE_UNIFORM], 0.8);
    assert_eq!(uniforms[GLASS_TRANSMISSION_REFRACTION_UNIFORM], 1.0);
    assert_eq!(uniforms[GLASS_EFFECT_DENSITY_UNIFORM], 1.0);
    assert_eq!(uniforms[GLASS_MENISCUS_ABSORPTION_UNIFORM], 0.3);
    assert_eq!(uniforms[18], 1.6);
    assert_eq!(uniforms[20], 0.12);
    assert_eq!(uniforms[21], 1.0);
    assert_eq!(uniforms[24], 1.05);
    assert_eq!(uniforms[GLASS_BLUR_RADIUS_UNIFORM], 6.0);
    assert_eq!(shader.input_padding(), 6.0);
}

#[test]
fn refraction_depth_is_clamped_at_the_shader_boundary() {
    for (input, expected) in [(-1.0, 0.0), (0.8, 0.8), (3.0, 2.0)] {
        let spec = LiquidGlassSpec {
            refraction_depth: input,
            ..LiquidGlassSpec::default()
        };
        let RenderEffect::Shader { shader } = liquid_glass_effect(&rect(), &spec, 800.0, 600.0)
        else {
            panic!("liquid glass must be one runtime shader");
        };
        assert_eq!(shader.uniforms()[9], expected);
    }
}

#[test]
fn refraction_curve_is_clamped_at_the_shader_boundary() {
    for (input, expected) in [(-1.0, 0.05), (0.8, 0.8), (3.0, 1.0)] {
        let spec = LiquidGlassSpec {
            refraction_curve: input,
            ..LiquidGlassSpec::default()
        };
        let RenderEffect::Shader { shader } = liquid_glass_effect(&rect(), &spec, 800.0, 600.0)
        else {
            panic!("liquid glass must be one runtime shader");
        };
        assert_eq!(shader.uniforms()[GLASS_REFRACTION_CURVE_UNIFORM], expected);
    }
}

#[test]
fn loupe_and_menu_use_the_same_wcksrd_program() {
    let loupe_spec = LiquidLoupeSpec::default();
    let RenderEffect::Shader { shader: loupe } = liquid_loupe_effect((117.0, 82.0), &loupe_spec)
    else {
        panic!("loupe must use the shared runtime shader");
    };
    assert_eq!(loupe.uniforms()[9], 0.34);
    assert_eq!(loupe.uniforms()[GLASS_REFRACTION_CURVE_UNIFORM], 0.25);
    assert_eq!(
        loupe.uniforms()[GLASS_DISPERSION_UNIFORM],
        loupe_spec.dispersion
    );
    assert_eq!(loupe.uniforms()[80], 1.0);
    assert!(loupe.input_padding() >= 75.0);

    let RenderEffect::Chain { first, second } = liquid_menu_glass_effect((240.0, 44.0), 8.0, 1.0)
    else {
        panic!("a heavy settled blur must chain a Gaussian into the shader");
    };
    let RenderEffect::Blur { radius_x, .. } = *first else {
        panic!("the chain's first stage is the Gaussian remainder");
    };
    assert!(radius_x > 0.0);
    let RenderEffect::Shader { shader: menu } = second.as_ref() else {
        panic!("the chain's second stage is the wcKSRD program");
    };
    assert_eq!(menu.uniforms()[9], 0.10);
    assert_eq!(menu.uniforms()[GLASS_REFRACTION_CURVE_UNIFORM], 0.25);
    assert!(menu.uniforms()[GLASS_BLUR_RADIUS_UNIFORM] <= 2.0 + 1.0e-6);
}

#[test]
fn liquid_glass_effect_multi_handles_empty_and_multiple_rects() {
    let spec = LiquidGlassSpec::default();
    assert!(liquid_glass_effect_multi(&[], &spec, 800.0, 600.0).is_none());
    let rects = [rect(), rect()];
    assert!(matches!(
        liquid_glass_effect_multi(&rects, &spec, 800.0, 600.0),
        Some(RenderEffect::Chain { .. })
    ));
}
