// Draws a composable's surface on a mesh in a 3D scene, with a depth test
// against the scene's other surfaces, and the depth of its shadow casters
// as the scene's light sees them.

struct MeshUniforms {
    clip_from_model: mat4x4<f32>,
    scene_from_model: mat4x4<f32>,
    // Maps the mesh into the shadow map: x and y from -1 to 1, depth from 0
    // nearest the light to 1.
    light_clip_from_model: mat4x4<f32>,
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
    // x = the surface's alpha; y = 1 for a lit surface, 0 for an unlit one.
    params: vec4<f32>,
    // x = one shadow map texel in texture coordinates; y = the depth bias.
    shadow: vec4<f32>,
}

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) scene_position: vec3<f32>,
    @location(3) light_position: vec3<f32>,
}

struct ShadowOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> mesh: MeshUniforms;
@group(2) @binding(0) var shadow_map: texture_depth_2d;
@group(2) @binding(1) var shadow_sampler: sampler_comparison;

@vertex
fn mesh_surface_vs(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let position = vec4<f32>(input.position, 1.0);
    output.clip_position = mesh.clip_from_model * position;
    output.scene_position = (mesh.scene_from_model * position).xyz;
    output.light_position = (mesh.light_clip_from_model * position).xyz;
    output.uv = input.uv;
    output.normal = vec3<f32>(
        dot(mesh.normal_x.xyz, input.normal),
        dot(mesh.normal_y.xyz, input.normal),
        dot(mesh.normal_z.xyz, input.normal),
    );
    return output;
}

// The content's colour at `uv`, read half a texel in from its edge so a
// surface that shares its texture never filters in a neighbour's texels.
fn content_color(uv: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(input_texture));
    let first = mesh.source_region.xy + vec2<f32>(0.5);
    let last = mesh.source_region.xy + mesh.source_region.zw - vec2<f32>(0.5);
    let texel = clamp(mesh.source_region.xy + uv * mesh.source_region.zw, first, last);
    return textureSample(input_texture, input_sampler, texel / size);
}

// How bright the surface is where the light reaches it by `visibility`, from
// 0 in full shadow to 1. The side the viewer sees takes the light that falls
// on that side.
fn shade(input: VertexOutput, visibility: f32) -> f32 {
    if (mesh.light.w < 0.0) {
        return 1.0;
    }
    var reach = 1.0;
    if (mesh.params.y > 0.5) {
        let toward_eye = mesh.eye.xyz - input.scene_position;
        let seen = select(-input.normal, input.normal, dot(input.normal, toward_eye) >= 0.0);
        let normal_length = length(seen);
        reach = select(0.0, max(0.0, -dot(seen, mesh.light.xyz)) / normal_length, normal_length > 0.0);
    }
    return mesh.light.w + (1.0 - mesh.light.w) * reach * visibility;
}

fn shaded(color: vec4<f32>, shade: f32) -> vec4<f32> {
    return vec4<f32>(color.rgb * shade, color.a) * mesh.params.x;
}

@fragment
fn mesh_surface_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let color = content_color(input.uv);
    if (color.a < 1.0 / 255.0) {
        discard;
    }
    return shaded(color, shade(input, 1.0));
}

// The share of the light that reaches a point at `position` in the shadow
// map, from four taps around it.
fn shadow_visibility(position: vec3<f32>) -> f32 {
    let uv = vec2<f32>(position.x * 0.5 + 0.5, 0.5 - position.y * 0.5);
    if (any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0))) {
        return 1.0;
    }
    let depth = clamp(position.z, 0.0, 1.0) - mesh.shadow.y;
    let step = mesh.shadow.x * 0.5;
    var lit = 0.0;
    lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(-step, -step), depth);
    lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(step, -step), depth);
    lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(-step, step), depth);
    lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + vec2<f32>(step, step), depth);
    return lit * 0.25;
}

@fragment
fn mesh_surface_shadowed_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let color = content_color(input.uv);
    if (color.a < 1.0 / 255.0) {
        discard;
    }
    return shaded(color, shade(input, shadow_visibility(input.light_position)));
}

@vertex
fn mesh_shadow_vs(input: VertexInput) -> ShadowOutput {
    var output: ShadowOutput;
    output.clip_position = mesh.light_clip_from_model * vec4<f32>(input.position, 1.0);
    output.uv = input.uv;
    return output;
}

// A caster's transparent texels cast no shadow.
@fragment
fn mesh_shadow_fs(input: ShadowOutput) {
    if (content_color(input.uv).a < 0.5) {
        discard;
    }
}
