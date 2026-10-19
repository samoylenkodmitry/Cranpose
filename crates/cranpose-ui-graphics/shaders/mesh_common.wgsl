// The part of a mesh surface's shader that every surface pipeline shares:
// the mesh's uniforms and vertex, its content, the shadow map and the cut
// a portal's window makes. `mesh_surface.wgsl` and the prelude of an app's
// surface shader (`mesh_shaded.wgsl`) each follow it.

struct MeshUniforms {
    clip_from_model: mat4x4<f32>,
    scene_from_model: mat4x4<f32>,
    // Maps the mesh into the shadow map: x and y from -1 to 1, depth from 0
    // nearest the light to 1.
    light_clip_from_model: mat4x4<f32>,
    // Maps the scene into the page's clip space, for glass's bent rays.
    clip_from_scene: mat4x4<f32>,
    // Rows of the inverse transpose of the model matrix's 3x3: turns model
    // normals into scene space.
    normal_x: vec4<f32>,
    normal_y: vec4<f32>,
    normal_z: vec4<f32>,
    // xyz = the eye in scene space.
    eye: vec4<f32>,
    // xyz = the direction the light travels, of length one; w = the ambient
    // share, or below zero for a scene without light.
    light: vec4<f32>,
    // xy = the texel where the content starts; zw = its size in texels.
    source_region: vec4<f32>,
    // x = the surface's alpha; y = 1 for a lit surface, 0 for an unlit one,
    // 2 for one that emits its colour, which neither light nor shadow
    // changes.
    params: vec4<f32>,
    // x = one shadow map texel in texture coordinates; y = the depth bias.
    shadow: vec4<f32>,
    // xy = how wide a penumbra is, in shadow map coordinates along each
    // axis, a unit of shadow map depth behind its caster; z > 0 for a light
    // of some size, whose shadows soften away from their casters.
    shadow_soft: vec4<f32>,
    // x = one over the glass's refractive index; y = how far a ray travels
    // inside it, in scene units; zw = the page's size in pixels.
    glass: vec4<f32>,
    // rgb = the colour the glass tints what shows through it.
    glass_tint: vec4<f32>,
    // xy = the page pixel the backdrop copy starts at; zw = its size.
    backdrop_region: vec4<f32>,
    // For a surface in a portal's space, the window's plane: a scene point
    // p lies beyond it where dot(xyz, p) + w >= 0.
    clip: vec4<f32>,
}

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> mesh: MeshUniforms;
@group(2) @binding(0) var shadow_map: texture_depth_2d;
@group(2) @binding(1) var shadow_sampler: sampler_comparison;
// The shadow map again, read as the depth of the nearest texel: GLSL
// samples a texture with one sampler, and reads a depth texture's depth
// only through a plain float sampler, which `texture_depth_2d` with a
// plain sampler is not.
@group(2) @binding(2) var shadow_depth: texture_2d<f32>;
@group(2) @binding(3) var shadow_depth_sampler: sampler;

// The content's colour at `uv`, read half a texel in from its edge so a
// surface that shares its texture never filters in a neighbour's texels.
fn content_color(uv: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(input_texture));
    let first = mesh.source_region.xy + vec2<f32>(0.5);
    let last = mesh.source_region.xy + mesh.source_region.zw - vec2<f32>(0.5);
    let texel = clamp(mesh.source_region.xy + uv * mesh.source_region.zw, first, last);
    return textureSample(input_texture, input_sampler, texel / size);
}

// Whether a point of a surface in a portal's space lies on the near side of
// its window's plane, where the space does not show.
fn before_window(position: vec3<f32>) -> bool {
    return dot(mesh.clip.xyz, position) + mesh.clip.w < 0.0;
}

// Sixteen points spread over the unit disc, which the shadow taps take.
fn disc_point(index: u32) -> vec2<f32> {
    var points = array<vec2<f32>, 16>(
        vec2<f32>(-0.9420, -0.3991), vec2<f32>(0.9456, -0.7689),
        vec2<f32>(-0.0942, -0.9294), vec2<f32>(0.3450, 0.2939),
        vec2<f32>(-0.9159, 0.4577), vec2<f32>(-0.8154, -0.8791),
        vec2<f32>(-0.3828, 0.2768), vec2<f32>(0.9748, 0.7565),
        vec2<f32>(0.4432, -0.9751), vec2<f32>(0.5374, -0.4737),
        vec2<f32>(-0.2650, -0.4189), vec2<f32>(0.7920, 0.1909),
        vec2<f32>(-0.2419, 0.9971), vec2<f32>(-0.8141, 0.9144),
        vec2<f32>(0.1998, 0.7864), vec2<f32>(0.1438, -0.1410),
    );
    return points[index];
}

// The share of the light that reaches a point at `position` in the shadow
// map. A light of no size takes four taps around the point; one of some
// size first finds how far the casters around the point lie in front of
// it, then filters over the penumbra that distance opens.
fn shadow_visibility(position: vec3<f32>) -> f32 {
    let uv = vec2<f32>(position.x * 0.5 + 0.5, 0.5 - position.y * 0.5);
    if (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0))) {
        return 1.0;
    }
    let depth = clamp(position.z, 0.0, 1.0) - mesh.shadow.y;
    if (mesh.shadow_soft.z <= 0.0) {
        let step = mesh.shadow.x * 0.5;
        var lit = 0.0;
        lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(-step, -step), depth);
        lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(step, -step), depth);
        lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(-step, step), depth);
        lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(step, step), depth);
        return lit * 0.25;
    }
    let search = mesh.shadow_soft.xy * depth;
    var blockers = 0.0;
    var blocker_depth = 0.0;
    for (var index = 0u; index < 16u; index++) {
        let found = textureSampleLevel(shadow_depth, shadow_depth_sampler, uv + disc_point(index) * search, 0.0).r;
        if (found < depth) {
            blockers += 1.0;
            blocker_depth += found;
        }
    }
    if (blockers == 0.0) {
        return 1.0;
    }
    let penumbra = max(mesh.shadow_soft.xy * (depth - blocker_depth / blockers), vec2<f32>(mesh.shadow.x));
    var lit = 0.0;
    for (var index = 0u; index < 16u; index++) {
        lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + disc_point(index) * penumbra, depth);
    }
    return lit / 16.0;
}
