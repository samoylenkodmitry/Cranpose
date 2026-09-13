fn blurred_disk(distance: f32, radius: f32) -> f32 {
    let z = 0.5 * pow(distance / radius, 2.0);
    let p = 0.3934693403 + z * (0.09020401043 + z * (0.007193838983
        + z * (0.0002919370927 + z * (0.000007171484581 + z * (0.0000001180411444
        + z * (1.392193893e-9 + z * (1.234065648e-11 + z * (8.520561128e-14
        + z * (4.711392277e-16 + z * (2.133168193e-18 + z * 8.053494527e-21))))))))));
    return exp(-z) * p;
}

@fragment
fn effect_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let source = textureSample(input_texture, input_sampler, input.uv);
    let scale = u[62].zw / max(u[0].xy, vec2<f32>(0.001));
    let position = (input.uv * vec2<f32>(textureDimensions(input_texture)) - u[62].xy) / scale;
    let half_size = u[0].xy * 0.5;
    let radius = min(half_size.x, half_size.y);
    let q = abs(position - half_size) - half_size + vec2<f32>(radius);
    let distance = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    let aa = 0.5 / max(min(scale.x, scale.y), 0.001);
    var coverage = 1.0 - smoothstep(-aa, aa, distance);
    if q.x <= 0.0 {
        coverage = select(0.0, 1.0, abs(position.y - half_size.y) <= half_size.y);
    }
    let luma = dot(source.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    let global = clamp((source.rgb - vec3<f32>(luma)) * 1.2 + vec3<f32>(luma + 0.05), vec3<f32>(0.0), vec3<f32>(1.0));
    let lit = mix(source.rgb, global, u[1].x);
    let lit_luma = dot(lit, vec3<f32>(0.2126, 0.7152, 0.0722));
    let local = clamp((lit - vec3<f32>(lit_luma)) * 2.7272727 + vec3<f32>(lit_luma * 1.8181818), vec3<f32>(0.0), vec3<f32>(1.0));
    let mask = blurred_disk(length(position - u[0].zw), u[1].z);
    return vec4<f32>(mix(lit, local, mask * u[1].y), source.a) * coverage;
}
