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
    // changes; z = 1 for a sheet whose outline is its texture's edge.
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
// The content has one level of detail, read explicitly, so a shader may
// read it inside a branch too.
fn content_color(uv: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(input_texture));
    let first = mesh.source_region.xy + vec2<f32>(0.5);
    let last = mesh.source_region.xy + mesh.source_region.zw - vec2<f32>(0.5);
    let texel = clamp(mesh.source_region.xy + uv * mesh.source_region.zw, first, last);
    return textureSampleLevel(input_texture, input_sampler, texel / size, 0.0);
}

// How much of the pixel at `uv` the surface covers: all of it, except along
// the outline of a sheet whose outline is its texture's edge, where it is
// the pixel's distance inside the edge, so the outline fades over a pixel
// at any angle instead of stepping from pixel to pixel.
fn outline_coverage(uv: vec2<f32>) -> f32 {
    let pixel = fwidth(uv);
    if (mesh.params.z < 0.5) {
        return 1.0;
    }
    let inside = min(uv, vec2<f32>(1.0) - uv) / max(pixel, vec2<f32>(1.0e-6));
    return clamp(min(inside.x, inside.y), 0.0, 1.0);
}

// Whether a point of a surface in a portal's space lies on the near side of
// its window's plane, where the space does not show.
fn before_window(position: vec3<f32>) -> bool {
    return dot(mesh.clip.xyz, position) + mesh.clip.w < 0.0;
}

// Point `index` of `count` spread evenly over the unit disc, the whole
// pattern turned by `turn` radians: the taps of a soft shadow.
fn disc_point(index: u32, count: u32, turn: f32) -> vec2<f32> {
    let angle = f32(index) * 2.399963 + turn;
    return sqrt((f32(index) + 0.5) / f32(count)) * vec2<f32>(cos(angle), sin(angle));
}

// How far the taps turn at the screen pixel `pixel`: interleaved gradient
// noise. One pattern for every pixel would draw each tap as a copy of the
// shadow's edge; turned from pixel to pixel, the taps fill the penumbra
// evenly.
fn tap_turn(pixel: vec2<f32>) -> f32 {
    let noise = fract(52.9829189 * fract(dot(floor(pixel), vec2<f32>(0.06711056, 0.00583715))));
    return noise * 6.2831853;
}

// The share of the light that reaches a point at `position` in the shadow
// map, seen at the screen pixel `pixel`. A light of no size takes four
// taps around the point; one of some size first finds how far the casters
// around the point lie in front of it, then filters over the penumbra that
// distance opens, with more taps for a wider penumbra.
fn shadow_visibility(position: vec3<f32>, pixel: vec2<f32>) -> f32 {
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
    let turn = tap_turn(pixel);
    let search = mesh.shadow_soft.xy * depth;
    var blockers = 0.0;
    var blocker_depth = 0.0;
    for (var index = 0u; index < 16u; index++) {
        let found = textureSampleLevel(shadow_depth, shadow_depth_sampler, uv + disc_point(index, 16u, turn) * search, 0.0).r;
        if (found < depth) {
            blockers += 1.0;
            blocker_depth += found;
        }
    }
    if (blockers == 0.0) {
        return 1.0;
    }
    let penumbra = max(mesh.shadow_soft.xy * (depth - blocker_depth / blockers), vec2<f32>(mesh.shadow.x));
    let texels = max(penumbra.x, penumbra.y) / mesh.shadow.x;
    let count = u32(clamp(texels, 16.0, 32.0));
    var lit = 0.0;
    for (var index = 0u; index < count; index++) {
        lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + disc_point(index, count, turn) * penumbra, depth);
    }
    return lit / f32(count);
}
