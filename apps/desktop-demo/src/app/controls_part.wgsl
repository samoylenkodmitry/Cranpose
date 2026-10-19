// A control's part on its card in the room, shaded as the controls' stage
// shades its parts: it mirrors the stage's lights and the card it stands
// on, tinted by its content, and darkens where the room's light is shadowed.
// It follows `controls_shading.wgsl`. The surface's values: 0 = how dark the
// stage is; 1 to 3 = the card's colour; 4 to 12 = the rows of the turn from
// the part's coordinates to the stage's lights', which the room's view of a
// card straight on takes to the controls tab's view of its stage.

fn turned_to_lights(r: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        dot(vec3<f32>(shaded_param(4u), shaded_param(5u), shaded_param(6u)), r),
        dot(vec3<f32>(shaded_param(7u), shaded_param(8u), shaded_param(9u)), r),
        dot(vec3<f32>(shaded_param(10u), shaded_param(11u), shaded_param(12u)), r),
    );
}

fn shade(surface: ShadedSurface) -> vec4<f32> {
    let dark = shaded_param(0u);
    let card = vec3<f32>(shaded_param(1u), shaded_param(2u), shaded_param(3u));
    let rd = -surface.to_eye;
    let n = surface.normal;
    let r = reflect(rd, n);
    let ground = ground_reflection(surface.position, r);
    let lights = environment(turned_to_lights(r), dark);
    let seen = mix(lights, srgb_to_linear(card) * mix(0.62, 0.40, dark), ground.z);
    let alpha = surface.color.a;
    let tint = surface.color.rgb / max(alpha, 1e-4);
    let lit = seen * tint * fresnel(n, rd) * mix(0.66, 1.0, surface.lit);
    return vec4<f32>(linear_to_srgb(tonemap(lit)) * alpha, alpha);
}
