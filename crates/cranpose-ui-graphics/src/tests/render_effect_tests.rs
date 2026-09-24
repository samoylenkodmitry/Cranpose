use std::cell::Cell;

use super::{RenderEffect, RuntimeShader};

#[test]
fn a_shader_declares_it_preserves_transparency() {
    let mut shader = RuntimeShader::new("// preserves");
    assert!(!shader.preserves_transparency());
    let plain = shader.clone();
    shader.set_preserves_transparency(true);
    assert!(shader.preserves_transparency());
    assert_ne!(shader, plain);
    shader.set_preserves_transparency(false);
    assert_eq!(shader, plain);
}

#[test]
fn an_effect_preserves_transparency_when_every_step_does() {
    let mut declared = RuntimeShader::new("// declared");
    declared.set_preserves_transparency(true);
    let undeclared = RuntimeShader::new("// undeclared");
    assert!(RenderEffect::blur(3.0).preserves_transparency());
    assert!(RenderEffect::offset(2.0, 1.0).preserves_transparency());
    assert!(RenderEffect::runtime_shader(declared.clone()).preserves_transparency());
    assert!(!RenderEffect::runtime_shader(undeclared.clone()).preserves_transparency());
    assert!(
        RenderEffect::blur(3.0)
            .then(RenderEffect::runtime_shader(declared))
            .preserves_transparency()
    );
    assert!(
        !RenderEffect::blur(3.0)
            .then(RenderEffect::runtime_shader(undeclared))
            .preserves_transparency()
    );
}

#[test]
fn overrides_stay_sorted_and_replace_by_name() {
    let mut shader = super::RuntimeShader::new("// overrides");
    shader.set_override("ZETA", 1.0);
    shader.set_override("ALPHA", 0.0);
    shader.set_override("ZETA", 2.0);
    assert_eq!(shader.overrides(), &[("ALPHA", 0.0), ("ZETA", 2.0)]);
}

#[test]
fn clear_override_removes_present_name_and_preserves_remaining_set() {
    let mut shader = super::RuntimeShader::new("");
    shader.set_override("ZETA", 1.0);
    shader.set_override("ALPHA", 2.0);
    let mut expected = super::RuntimeShader::new("");
    expected.set_override("ALPHA", 2.0);
    assert!(shader.clear_override("ZETA"));
    assert!(!shader.clear_override("MISSING"));
    assert_eq!(shader.overrides(), &[("ALPHA", 2.0)]);
    assert_eq!(shader.overrides_hash(), expected.overrides_hash());
    assert!(shader.clear_override("ALPHA"));
    assert!(shader.overrides().is_empty());
    assert_eq!(shader.overrides_hash(), 0);
}

#[test]
fn overrides_distinguish_otherwise_equal_shaders() {
    let plain = super::RuntimeShader::new("// overrides-eq");
    let mut raised = plain.clone();
    raised.set_override("FLAG", 1.0);
    assert_eq!(plain.overrides_hash(), 0);
    assert_ne!(plain.overrides_hash(), raised.overrides_hash());
    assert_ne!(plain, raised);
    let mut lowered = raised.clone();
    lowered.set_override("FLAG", 0.0);
    assert_ne!(raised.overrides_hash(), lowered.overrides_hash());
    assert_ne!(raised, lowered);
}

#[test]
fn unchanged_override_lookups_share_one_hash_computation() {
    let mut shader = RuntimeShader::new("");
    shader.set_override("FLAG", 1.0);
    shader.set_override("SCALE", 0.5);
    OVERRIDE_HASH_COMPUTATIONS.with(|count| count.set(0));
    let expected = shader.overrides_hash();
    for _ in 0..24 {
        let mut copy = shader.clone();
        copy.set_float(0, 7.0);
        copy.set_override("FLAG", 1.0);
        assert!(!copy.clear_override("ABSENT"));
        assert_eq!(copy.overrides_hash(), expected);
    }
    assert_eq!(OVERRIDE_HASH_COMPUTATIONS.with(Cell::get), 1);
}

#[test]
fn override_hash_tracks_clone_mutations_and_float_bits() {
    fn independent_hash(shader: &RuntimeShader) -> u64 {
        let bytes: Vec<u8> = shader
            .overrides()
            .iter()
            .flat_map(|(name, value)| name.bytes().chain([0]).chain(value.to_bits().to_le_bytes()))
            .collect();
        if bytes.is_empty() {
            0
        } else {
            hash_shader_bytes(bytes)
        }
    }

    let mut original = RuntimeShader::new("");
    original.set_override("VALUE", 0.0);
    let first = original.overrides_hash();
    assert_eq!(first, independent_hash(&original));
    for value in [
        -0.0,
        0.5,
        f64::INFINITY,
        f64::from_bits(0x7ff8_0000_0000_0001),
    ] {
        let mut changed = original.clone();
        changed.set_override("VALUE", value);
        assert_eq!(changed.overrides_hash(), independent_hash(&changed));
        assert_ne!(changed.overrides_hash(), first);
        changed.set_override("ADDED", 1.0);
        assert_eq!(changed.overrides_hash(), independent_hash(&changed));
        assert!(changed.clear_override("VALUE"));
        assert_eq!(changed.overrides_hash(), independent_hash(&changed));
        assert!(changed.clear_override("ADDED"));
        assert_eq!(changed.overrides_hash(), 0);
        assert_eq!(original.overrides_hash(), first);
    }
}

#[test]
fn shader_clones_share_declarations_and_isolate_mutation() {
    let mut shader = RuntimeShader::new("fn effect_fs() {}");
    shader.set_override("FLAG", 1.0);
    shader.set_substrates(&[SubstrateSpec::Average { block: 4 }]);
    shader.set_draw_split(Some("SPLIT"));
    let support = Rect {
        x: 1.0,
        y: 2.0,
        width: 30.0,
        height: 40.0,
    };
    shader.set_output_support(Some(support));
    let mut cloned = shader.clone();
    assert_eq!(cloned.overrides().as_ptr(), shader.overrides().as_ptr());
    assert_eq!(cloned.substrates().as_ptr(), shader.substrates().as_ptr());
    cloned.set_float(0, 2.0);
    assert!(shader.uniforms().is_empty());
    assert_eq!(cloned.overrides().as_ptr(), shader.overrides().as_ptr());
    cloned.set_override("FLAG", 2.0);
    assert_eq!(cloned.substrates(), shader.substrates());
    assert_eq!(cloned.draw_split(), shader.draw_split());
    cloned.set_substrates(&[SubstrateSpec::Average { block: 8 }]);
    cloned.set_draw_split(None);
    cloned.set_output_support(None);
    assert_eq!(shader.overrides(), &[("FLAG", 1.0)]);
    assert_eq!(shader.substrates(), &[SubstrateSpec::Average { block: 4 }]);
    assert_eq!(shader.draw_split(), Some("SPLIT"));
    assert_eq!(shader.output_support(), Some(support));
    assert_ne!(cloned, shader);
}

use super::*;
use crate::RoundedCornerShape;

#[test]
fn cloned_effect_chains_keep_order_and_isolate_nested_edits() {
    let original = RenderEffect::offset(2.0, 7.0)
        .then(RenderEffect::blur(3.0))
        .then(RenderEffect::offset(-4.0, 1.0));
    let mut edited = original.clone();
    assert_eq!(edited, original);
    let RenderEffect::Chain {
        first: original_first,
        second: original_second,
    } = &original
    else {
        panic!("chain effect")
    };
    let RenderEffect::Chain {
        first: edited_first,
        second: edited_second,
    } = &mut edited
    else {
        panic!("chain effect")
    };
    assert!(Arc::ptr_eq(original_first, edited_first));
    assert!(Arc::ptr_eq(original_second, edited_second));
    assert_eq!(original_second.as_ref(), &RenderEffect::offset(-4.0, 1.0));
    let RenderEffect::Chain { first, second } = Arc::make_mut(edited_first) else {
        panic!("nested chain")
    };
    assert_eq!(first.as_ref(), &RenderEffect::offset(2.0, 7.0));
    assert_eq!(second.as_ref(), &RenderEffect::blur(3.0));
    *Arc::make_mut(first) = RenderEffect::offset(12.0, 17.0);
    *Arc::make_mut(edited_second) = RenderEffect::blur(11.0);
    assert_eq!(
        original,
        RenderEffect::offset(2.0, 7.0)
            .then(RenderEffect::blur(3.0))
            .then(RenderEffect::offset(-4.0, 1.0))
    );
    assert_eq!(
        edited,
        RenderEffect::offset(12.0, 17.0)
            .then(RenderEffect::blur(3.0))
            .then(RenderEffect::blur(11.0))
    );
}

#[test]
fn cloned_shader_effects_preserve_configuration_and_isolate_edits() {
    let mut shader = RuntimeShader::new("fn effect_fs() {}");
    shader.set_float(20, 3.0);
    shader.set_override("FEATURE", -0.0);
    shader.set_input_padding(7.0);
    shader.set_substrates(&[SubstrateSpec::Blur { radius_px: 12.0 }]);
    shader.set_draw_split(Some("SPLIT"));
    let original = RenderEffect::runtime_shader(shader.clone());
    let mut edited = original.clone();
    assert_eq!(edited, original);
    let RenderEffect::Shader {
        shader: original_shader,
    } = &original
    else {
        panic!("shader effect")
    };
    assert_eq!(original_shader.as_ref(), &shader);
    let RenderEffect::Shader {
        shader: edited_shader,
    } = &mut edited
    else {
        panic!("shader effect")
    };
    assert!(Arc::ptr_eq(original_shader, edited_shader));
    let changed = Arc::make_mut(edited_shader);
    changed.set_float(20, 9.0);
    changed.set_override("FEATURE", 1.0);
    changed.set_substrates(&[]);
    changed.set_draw_split(None);
    assert_eq!(original_shader.as_ref(), &shader);
    assert_eq!(edited_shader.uniforms()[20], 9.0);
    assert_eq!(edited_shader.overrides(), &[("FEATURE", 1.0)]);
    assert!(edited_shader.substrates().is_empty());
    assert_eq!(edited_shader.draw_split(), None);
    assert_ne!(original, edited);
}

#[test]
fn runtime_shader_set_uniforms() {
    let mut shader = RuntimeShader::new("// test");
    shader.set_float(0, 1.0);
    shader.set_float2(2, 3.0, 4.0);
    shader.set_float4(4, 5.0, 6.0, 7.0, 8.0);

    assert_eq!(shader.uniforms()[0], 1.0);
    assert_eq!(shader.uniforms()[1], 0.0);
    assert_eq!(shader.uniforms()[2], 3.0);
    assert_eq!(shader.uniforms()[3], 4.0);
    assert_eq!(shader.uniforms()[4], 5.0);
    assert_eq!(shader.uniforms()[5], 6.0);
    assert_eq!(shader.uniforms()[6], 7.0);
    assert_eq!(shader.uniforms()[7], 8.0);
}

#[test]
fn runtime_shader_padded() {
    let mut shader = RuntimeShader::new("// test");
    shader.set_float(0, 42.0);
    let padded = shader.uniforms_padded();
    assert_eq!(padded[0], 42.0);
    assert_eq!(padded[1], 0.0);
    assert_eq!(padded[255], 0.0);
}

#[test]
fn blur_and_offset_declare_input_padding() {
    assert_eq!(
        RenderEffect::blur_xy(6.0, 12.0, TileMode::Clamp).input_padding(),
        12.0
    );
    assert_eq!(RenderEffect::offset(-8.0, 3.0).input_padding(), 8.0);
}

#[test]
fn chained_effect_padding_accumulates_sampling_ranges() {
    let mut shader = RuntimeShader::new("// test");
    shader.set_input_padding(9.0);
    let effect = RenderEffect::blur_xy(4.0, 6.0, TileMode::Clamp)
        .then(RenderEffect::runtime_shader(shader))
        .then(RenderEffect::offset(2.0, -5.0));

    assert_eq!(effect.input_padding(), 20.0);
}

#[test]
fn runtime_shader_keeps_common_uniform_payload_inline() {
    let mut shader = RuntimeShader::new("// test");
    shader.set_float4(0, 1.0, 2.0, 3.0, 4.0);
    shader.set_float4(4, 5.0, 6.0, 7.0, 8.0);
    shader.set_float4(8, 9.0, 10.0, 11.0, 12.0);
    shader.set_float4(12, 13.0, 14.0, 15.0, 16.0);

    assert!(shader.uniforms.is_inline());
    assert_eq!(shader.uniforms().len(), 16);

    shader.set_float(16, 17.0);
    assert!(!shader.uniforms.is_inline());
    assert_eq!(shader.uniforms()[16], 17.0);
}

#[test]
fn runtime_shader_try_set_reports_reserved_uniform_slots() {
    let mut shader = RuntimeShader::new("// test");

    let err = shader
        .try_set_float(RuntimeShader::RESERVED_UNIFORM_START, 1.0)
        .unwrap_err();
    assert_eq!(
        err,
        RuntimeShaderUniformError::OutOfUserRange {
            index: RuntimeShader::RESERVED_UNIFORM_START,
            width: 1,
            max_user_uniforms: RuntimeShader::MAX_USER_UNIFORMS,
            reserved_start: RuntimeShader::RESERVED_UNIFORM_START,
            max_uniforms: RuntimeShader::MAX_UNIFORMS,
        }
    );
    assert!(shader.uniforms().is_empty());

    let err = shader
        .try_set_float4(RuntimeShader::MAX_USER_UNIFORMS - 3, 1.0, 2.0, 3.0, 4.0)
        .unwrap_err();
    assert_eq!(
        err,
        RuntimeShaderUniformError::OutOfUserRange {
            index: RuntimeShader::MAX_USER_UNIFORMS - 3,
            width: 4,
            max_user_uniforms: RuntimeShader::MAX_USER_UNIFORMS,
            reserved_start: RuntimeShader::RESERVED_UNIFORM_START,
            max_uniforms: RuntimeShader::MAX_UNIFORMS,
        }
    );
}

#[test]
fn runtime_shader_setters_ignore_invalid_uniform_slots_without_panicking() {
    let mut shader = RuntimeShader::new("// test");
    shader.set_float(0, 7.0);

    shader.set_float(RuntimeShader::RESERVED_UNIFORM_START, 1.0);
    shader.set_float4(RuntimeShader::MAX_USER_UNIFORMS - 3, 1.0, 2.0, 3.0, 4.0);

    assert_eq!(shader.uniforms(), &[7.0]);
}

#[test]
fn render_effect_chaining() {
    let blur = RenderEffect::blur(10.0);
    let offset = RenderEffect::offset(5.0, 5.0);
    let chained = blur.then(offset);
    match chained {
        RenderEffect::Chain { first, second } => {
            assert!(matches!(*first, RenderEffect::Blur { .. }));
            assert!(matches!(*second, RenderEffect::Offset { .. }));
        }
        _ => panic!("expected Chain"),
    }
}

#[test]
fn blur_convenience() {
    let effect = RenderEffect::blur(15.0);
    match effect {
        RenderEffect::Blur {
            radius_x,
            radius_y,
            edge_treatment,
        } => {
            assert_eq!(radius_x, 15.0);
            assert_eq!(radius_y, 15.0);
            assert_eq!(edge_treatment, TileMode::Clamp);
        }
        _ => panic!("expected Blur"),
    }
}

#[test]
fn blur_with_edge_treatment_uses_explicit_mode() {
    let effect = RenderEffect::blur_with_edge_treatment(6.0, TileMode::Decal);
    match effect {
        RenderEffect::Blur {
            radius_x,
            radius_y,
            edge_treatment,
        } => {
            assert_eq!(radius_x, 6.0);
            assert_eq!(radius_y, 6.0);
            assert_eq!(edge_treatment, TileMode::Decal);
        }
        _ => panic!("expected Blur"),
    }
}

#[test]
fn source_hash_consistent() {
    let s1 = RuntimeShader::new("fn main() {}");
    let s2 = RuntimeShader::new("fn main() {}");
    assert_eq!(s1.source_hash(), s2.source_hash());
}

#[test]
fn runtime_shader_from_shared_source_reuses_shared_source() {
    let source = Arc::<str>::from("fn fragment() -> vec4<f32> { return vec4<f32>(1.0); }");
    let s1 = RuntimeShader::from_shared_source(source.clone());
    let s2 = RuntimeShader::from_shared_source(source);

    assert!(Arc::ptr_eq(&s1.source, &s2.source));
    assert_eq!(s1.source_hash(), s2.source_hash());
}

fn runtime_shader_from_reuse_callsite(source: &str) -> RuntimeShader {
    RuntimeShader::new(source)
}

fn runtime_shader_from_replacement_callsite(source: &str) -> RuntimeShader {
    RuntimeShader::new(source)
}

#[test]
fn runtime_shader_new_reuses_same_callsite_source() {
    let source = "fn fragment() -> vec4<f32> { return vec4<f32>(1.0); }";
    let s1 = runtime_shader_from_reuse_callsite(source);
    let s2 = runtime_shader_from_reuse_callsite(source);

    assert!(Arc::ptr_eq(&s1.source, &s2.source));
    assert_eq!(s1.source_hash(), s2.source_hash());
}

#[test]
fn runtime_shader_new_replaces_changed_callsite_source() {
    let s1 = runtime_shader_from_replacement_callsite("fn a() {}");
    let s2 = runtime_shader_from_replacement_callsite("fn b() {}");

    assert!(!Arc::ptr_eq(&s1.source, &s2.source));
    assert_ne!(s1.source_hash(), s2.source_hash());
    assert_eq!(s2.source(), "fn b() {}");
}

#[test]
fn runtime_shader_source_storage_has_no_process_global_interner() {
    let source = include_str!("../render_effect.rs");
    let blocked_static = ["static ", "INTERNER"].concat();
    let blocked_type = ["ShaderSource", "Interner"].concat();

    assert!(
        !source.contains(&blocked_static) && !source.contains(&blocked_type),
        "RuntimeShader source sharing must be explicit via from_shared_source, not a process-global interner"
    );
}

#[test]
fn blur_xy_preserves_tile_mode() {
    let effect = RenderEffect::blur_xy(3.0, 7.0, TileMode::Clamp);
    match effect {
        RenderEffect::Blur {
            radius_x,
            radius_y,
            edge_treatment,
        } => {
            assert_eq!(radius_x, 3.0);
            assert_eq!(radius_y, 7.0);
            assert_eq!(edge_treatment, TileMode::Clamp);
        }
        _ => panic!("expected Blur"),
    }
}

#[test]
fn offset_constructor_sets_components() {
    let effect = RenderEffect::offset(11.0, -5.0);
    match effect {
        RenderEffect::Offset { offset_x, offset_y } => {
            assert_eq!(offset_x, 11.0);
            assert_eq!(offset_y, -5.0);
        }
        _ => panic!("expected Offset"),
    }
}

#[test]
fn runtime_shader_equality_is_source_value_based() {
    let mut s1 = RuntimeShader::new("fn main() {}");
    let mut s2 = RuntimeShader::new("fn main() {}");
    s1.set_float(0, 1.0);
    s2.set_float(0, 1.0);
    assert_eq!(s1, s2);
}

#[test]
fn blurred_edge_treatment_defaults_to_bounded_rectangle() {
    let treatment = BlurredEdgeTreatment::default();
    assert_eq!(treatment.shape(), Some(LayerShape::Rectangle));
    assert!(treatment.clip());
    assert_eq!(treatment.tile_mode(), TileMode::Clamp);
}

#[test]
fn blurred_edge_treatment_unbounded_uses_decal_and_no_clip() {
    let treatment = BlurredEdgeTreatment::UNBOUNDED;
    assert_eq!(treatment.shape(), None);
    assert!(!treatment.clip());
    assert_eq!(treatment.tile_mode(), TileMode::Decal);
}

#[test]
fn blurred_edge_treatment_with_shape_uses_bounded_mode() {
    let rounded = LayerShape::Rounded(RoundedCornerShape::uniform(8.0));
    let treatment = BlurredEdgeTreatment::with_shape(rounded);
    assert_eq!(treatment.shape(), Some(rounded));
    assert!(treatment.clip());
    assert_eq!(treatment.tile_mode(), TileMode::Clamp);
}

#[test]
fn an_effect_chains_output_support_is_the_support_of_the_stage_that_writes_its_output() {
    let mut shader = RuntimeShader::new("fn glass_fs() {}");
    assert_eq!(shader.output_support(), None);
    let support = Rect {
        x: 4.0,
        y: 6.0,
        width: 30.0,
        height: 12.0,
    };
    shader.set_output_support(Some(support));
    assert_eq!(shader.output_support(), Some(support));
    let effect = RenderEffect::blur(3.0).then(RenderEffect::runtime_shader(shader.clone()));
    assert_eq!(effect.output_support(), Some(support));
    let effect = RenderEffect::runtime_shader(shader.clone()).then(RenderEffect::blur(3.0));
    assert_eq!(effect.output_support(), None);
    assert_eq!(RenderEffect::blur(3.0).output_support(), None);
}

#[test]
fn a_sample_domain_is_the_writers_and_a_blur_declares_none() {
    let mut shader = RuntimeShader::new("fn glass_fs() {}");
    let domain = Rect {
        x: -2.0,
        y: -2.0,
        width: 20.0,
        height: 12.0,
    };
    let plain = shader.clone();
    shader.set_sample_domain(Some(domain));
    assert_ne!(shader, plain);
    assert_eq!(shader.sample_domain(), Some(domain));
    let effect = RenderEffect::blur(3.0).then(RenderEffect::runtime_shader(shader.clone()));
    assert_eq!(effect.sample_domain(), Some(domain));
    assert_eq!(RenderEffect::blur(3.0).output_support(), None);
    assert_eq!(RenderEffect::blur(3.0).sample_domain(), None);
    shader.set_sample_domain(Some(Rect {
        x: f32::INFINITY,
        ..domain
    }));
    assert_eq!(shader.sample_domain(), None);
}

#[test]
fn a_non_finite_output_support_clears_the_declaration_and_a_support_tells_shaders_apart() {
    let mut shader = RuntimeShader::new("fn glass_fs() {}");
    let plain = shader.clone();
    shader.set_output_support(Some(Rect {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 10.0,
    }));
    assert_ne!(shader, plain);
    shader.set_output_support(Some(Rect {
        x: 0.0,
        y: 0.0,
        width: f32::NAN,
        height: 10.0,
    }));
    assert_eq!(shader.output_support(), None);
    assert_eq!(shader, plain);
}
#[test]
fn specialization_cache_preserves_source_identity_and_shader_values() {
    let mut cache = ShaderSpecializationCache::<u32, 2>::new();
    let mut first = RuntimeShader::new("fn effect_fs() {}");
    first.set_override("CALLER", -0.0);
    let mut second = first.clone();
    second.set_override("CALLER", f64::from_bits(0x7ff8_0000_0000_0001));
    let sources = [first, second];
    for key in [1, 1, 2, 3, 1] {
        for source in &sources {
            let mut shader = source.clone();
            shader.set_float(0, key as f32);
            shader.set_input_padding(key as f32);
            cache.apply(&mut shader, key, |shader, &key| {
                shader.set_override("FEATURE", f64::from(key));
                shader.set_draw_split(Some("SPLIT"));
                shader.set_substrates(&[SubstrateSpec::Average { block: key }]);
            });
            assert_eq!(
                shader.overrides()[0].1.to_bits(),
                source.overrides()[0].1.to_bits()
            );
            assert_eq!(shader.overrides()[1], ("FEATURE", f64::from(key)));
            assert_eq!(
                shader.substrates(),
                &[SubstrateSpec::Average { block: key }]
            );
            assert_eq!(shader.draw_split(), Some("SPLIT"));
            assert_eq!(shader.uniforms(), &[key as f32]);
            assert_eq!(shader.input_padding(), key as f32);
            assert_eq!(source.overrides().len(), 1);
            assert!(source.substrates().is_empty());
            assert_eq!(source.draw_split(), None);
            let mut repeated = source.clone();
            cache.apply(&mut repeated, key, |_, _| {
                panic!("shared specialization missed")
            });
            assert_eq!(repeated.overrides_hash(), shader.overrides_hash());
            assert!(Arc::ptr_eq(
                repeated.specialization.as_ref().unwrap(),
                shader.specialization.as_ref().unwrap(),
            ));
            assert!(cache.entries.len() <= 2);
        }
    }
}

#[test]
fn specialization_cache_mutates_unique_state_without_retaining_it() {
    let mut cache = ShaderSpecializationCache::<(), 2>::new();
    let mut shader = RuntimeShader::new("fn effect_fs() {}");
    shader.set_override("VALUE", 1.0);
    let allocation = Arc::as_ptr(shader.specialization.as_ref().unwrap());
    cache.apply(&mut shader, (), |shader, ()| {
        shader.set_override("VALUE", 2.0);
    });
    assert_eq!(shader.overrides(), &[("VALUE", 2.0)]);
    assert_eq!(
        Arc::as_ptr(shader.specialization.as_ref().unwrap()),
        allocation
    );
    assert!(cache.entries.is_empty());
}

#[test]
fn unchanged_shader_declarations_keep_their_storage() {
    let mut shader = RuntimeShader::new("fn effect_fs() {}");
    shader.set_substrates(&[]);
    shader.set_draw_split(None);
    assert!(!shader.clear_override("MISSING"));
    assert!(shader.specialization.is_none());
    shader.set_override("FLAG", 1.0);
    let mut cloned = shader.clone();
    cloned.set_override("FLAG", 1.0);
    cloned.set_substrates(&[]);
    cloned.set_draw_split(None);
    assert!(!cloned.clear_override("MISSING"));
    assert_eq!(cloned.overrides().as_ptr(), shader.overrides().as_ptr());
}

#[test]
fn mean_substrates_have_distinct_stable_identity() {
    use std::hash::{DefaultHasher, Hasher};
    let hash = |spec: SubstrateSpec| {
        let mut h = DefaultHasher::new();
        spec.hash_bits(&mut h);
        h.finish()
    };
    let mean = SubstrateSpec::Mean;
    assert!(mean.same_bits(&mean));
    for other in [
        SubstrateSpec::Average { block: 4 },
        SubstrateSpec::Blur { radius_px: 12.0 },
    ] {
        assert!(!mean.same_bits(&other));
        assert_ne!(hash(mean), hash(other));
    }
    let mut shader = RuntimeShader::new("fn effect_fs() {}");
    shader.set_substrates(&[mean]);
    let mut cloned = shader.clone();
    cloned.set_substrates(&[SubstrateSpec::Average { block: 4 }]);
    assert_eq!(shader.substrates(), &[mean]);
    assert_eq!(cloned.substrates(), &[SubstrateSpec::Average { block: 4 }]);
}

#[test]
fn shader_substrates_preserve_order_and_ownership_across_size_changes() {
    let declared = [
        SubstrateSpec::Blur { radius_px: 12.0 },
        SubstrateSpec::Average { block: 4 },
        SubstrateSpec::Blur { radius_px: -0.0 },
    ];
    let mut source = declared;
    let mut original = RuntimeShader::new("fn effect_fs() {}");
    original.set_substrates(&source);
    source[0] = SubstrateSpec::Average { block: 16 };
    let mut changed = original.clone();
    for replacement in [&source[..1], &source[..2], &source[..0], &source[..]] {
        changed.set_substrates(replacement);
        assert_eq!(changed.substrates().len(), replacement.len());
        assert!(
            changed
                .substrates()
                .iter()
                .zip(replacement)
                .all(|(actual, expected)| actual.same_bits(expected))
        );
        assert_eq!(original.substrates().len(), declared.len());
        assert!(
            original
                .substrates()
                .iter()
                .zip(&declared)
                .all(|(actual, expected)| actual.same_bits(expected))
        );
    }
}

#[test]
fn shader_substrate_setters_preserve_float_bits_when_detaching() {
    let mut original = RuntimeShader::new("fn effect_fs() {}");
    original.set_substrates(&[SubstrateSpec::Blur { radius_px: 0.0 }]);
    let mut cloned = original.clone();
    cloned.set_substrates(&[SubstrateSpec::Blur { radius_px: 0.0 }]);
    assert_eq!(cloned.substrates().as_ptr(), original.substrates().as_ptr());
    cloned.set_substrates(&[SubstrateSpec::Blur { radius_px: -0.0 }]);
    let [SubstrateSpec::Blur { radius_px }] = cloned.substrates() else {
        panic!("one blur substrate");
    };
    assert_eq!(radius_px.to_bits(), (-0.0_f32).to_bits());
    let [SubstrateSpec::Blur { radius_px }] = original.substrates() else {
        panic!("original blur substrate");
    };
    assert_eq!(radius_px.to_bits(), 0.0_f32.to_bits());
}
