// The part of a shaded surface's shader the framework writes, after
// `mesh_common.wgsl`: a mesh in a 3D scene whose pixels each ask the app's
// `shade` function for their colour. The app's WGSL follows this and
// defines
//
//     fn shade(surface: ShadedSurface) -> vec4<f32>
//
// which returns the colour, premultiplied, from what `ShadedSurface` holds
// in the mesh's own coordinates. It reads its values with
// `shaded_param(index)`.

struct ShadedUniforms {
    // xyz = the eye in the mesh's coordinates.
    eye: vec4<f32>,
    // xyz = the direction the light travels in the mesh's coordinates, of
    // length one; w = its ambient share, below zero for a scene without
    // light.
    light: vec4<f32>,
    params: array<vec4<f32>, 8>,
}

// A point of the surface, in the mesh's own coordinates.
struct ShadedSurface {
    // The content's colour there, premultiplied.
    color: vec4<f32>,
    position: vec3<f32>,
    // Of length one, on the side the eye sees.
    normal: vec3<f32>,
    // Toward the eye, of length one.
    to_eye: vec3<f32>,
    // Toward the light, of length one; zero without a light.
    to_light: vec3<f32>,
    // How much of the light reaches the point: 0 in full shadow, 1 lit.
    lit: f32,
    // How bright a point the light misses is: 1 without a light.
    ambient: f32,
}

struct ShadedVaryings {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) position: vec3<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) light_position: vec3<f32>,
}

@group(3) @binding(0) var<uniform> shaded: ShadedUniforms;

fn shaded_param(index: u32) -> f32 {
    return shaded.params[index / 4u][index % 4u];
}

@vertex
fn mesh_shaded_vs(input: VertexInput) -> ShadedVaryings {
    var output: ShadedVaryings;
    let position = vec4<f32>(input.position, 1.0);
    output.clip_position = mesh.clip_from_model * position;
    output.light_position = (mesh.light_clip_from_model * position).xyz;
    output.uv = input.uv;
    output.position = input.position;
    output.normal = input.normal;
    return output;
}

// The point the app's shader colours: `color` of the content and
// `visibility` of the light reach it.
fn shaded_surface(input: ShadedVaryings, color: vec4<f32>, visibility: f32) -> ShadedSurface {
    var surface: ShadedSurface;
    surface.color = color;
    surface.position = input.position;
    surface.to_eye = normalize(shaded.eye.xyz - input.position);
    let normal = normalize(input.normal);
    surface.normal = select(-normal, normal, dot(normal, surface.to_eye) >= 0.0);
    let lit_scene = shaded.light.w >= 0.0;
    surface.to_light = select(vec3<f32>(0.0), -shaded.light.xyz, lit_scene);
    surface.lit = select(1.0, visibility, lit_scene);
    surface.ambient = select(1.0, shaded.light.w, lit_scene);
    return surface;
}

@fragment
fn mesh_shaded_fs(input: ShadedVaryings) -> @location(0) vec4<f32> {
    let color = content_color(input.uv);
    if (color.a < 1.0 / 255.0) {
        discard;
    }
    return shade(shaded_surface(input, color, 1.0)) * mesh.params.x;
}

@fragment
fn mesh_shaded_shadowed_fs(input: ShadedVaryings) -> @location(0) vec4<f32> {
    let color = content_color(input.uv);
    if (color.a < 1.0 / 255.0) {
        discard;
    }
    let visibility = shadow_visibility(input.light_position);
    return shade(shaded_surface(input, color, visibility)) * mesh.params.x;
}

// A shaded surface in a portal's space, which shows only beyond its
// window's plane.
@fragment
fn mesh_shaded_portal_fs(input: ShadedVaryings) -> @location(0) vec4<f32> {
    let scene_position = (mesh.scene_from_model * vec4<f32>(input.position, 1.0)).xyz;
    let color = content_color(input.uv);
    if (before_window(scene_position) | (color.a < 1.0 / 255.0)) {
        discard;
    }
    return shade(shaded_surface(input, color, 1.0)) * mesh.params.x;
}
