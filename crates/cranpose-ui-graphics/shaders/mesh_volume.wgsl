// The part of a volume's shader the framework writes: a mesh in a 3D scene
// whose pixels each ask the app's `volume` function what the camera's ray
// through the pixel meets inside the mesh, and take the depth of what it
// meets. The app's WGSL follows this and defines
//
//     fn volume(eye: vec3<f32>, direction: vec3<f32>, entry: f32, exit: f32, footprint: f32) -> VolumeHit
//
// in the mesh's own coordinates: the eye; the ray's direction, of length
// one; how far along the ray it enters the mesh's bounds, zero for an eye
// inside them, and leaves the mesh; and how wide a pixel is, in radians. It
// reads its values with `volume_param(index)`, the surface's content with
// `volume_content(uv)` and the scene's light with `volume_light()`.

struct VolumeUniforms {
    clip_from_model: mat4x4<f32>,
    // xyz = the eye in the mesh's coordinates.
    eye: vec4<f32>,
    // xyz = the direction the scene's light travels in the mesh's
    // coordinates, of length one; w = its ambient share, below zero for a
    // scene without light.
    light: vec4<f32>,
    // x = a pixel's width in radians; y = the surface's alpha.
    footprint: vec4<f32>,
    // The corners of the mesh's bounds.
    low: vec4<f32>,
    high: vec4<f32>,
    params: array<vec4<f32>, 8>,
}

// What a ray meets: its colour, premultiplied, nothing at an alpha of zero;
// and where, in the mesh's coordinates.
struct VolumeHit {
    color: vec4<f32>,
    at: vec3<f32>,
}

struct VolumeVertex {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
}

struct VolumeVaryings {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) model_position: vec3<f32>,
    @location(1) normal: vec3<f32>,
}

struct VolumeFragment {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

@group(0) @binding(0) var volume_texture: texture_2d<f32>;
@group(0) @binding(1) var volume_sampler: sampler;
@group(1) @binding(0) var<uniform> volume_mesh: VolumeUniforms;

// The value the app set at `index`, from 0 to 31.
fn volume_param(index: u32) -> f32 {
    return volume_mesh.params[index / 4u][index % 4u];
}

// The surface's content at `uv`, from (0, 0) at its top left to (1, 1).
fn volume_content(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(volume_texture, volume_sampler, uv, 0.0);
}

// The direction the scene's light travels and its ambient share, in the
// mesh's coordinates.
fn volume_light() -> vec4<f32> {
    return volume_mesh.light;
}

// How far along the ray from `eye` it enters the mesh's bounds, zero for an
// eye inside them.
fn volume_entry(eye: vec3<f32>, direction: vec3<f32>) -> f32 {
    let along = select(direction, vec3<f32>(1e-6), abs(direction) < vec3<f32>(1e-6));
    let to_low = (volume_mesh.low.xyz - eye) / along;
    let to_high = (volume_mesh.high.xyz - eye) / along;
    let enters = min(to_low, to_high);
    return max(max(enters.x, max(enters.y, enters.z)), 0.0);
}

@vertex
fn mesh_volume_vs(input: VolumeVertex) -> VolumeVaryings {
    var output: VolumeVaryings;
    output.clip_position = volume_mesh.clip_from_model * vec4<f32>(input.position, 1.0);
    output.model_position = input.position;
    output.normal = input.normal;
    return output;
}

// Each pixel asks once, from the side of the mesh that faces away from the
// eye, which an eye inside the mesh sees as well.
@fragment
fn mesh_volume_fs(input: VolumeVaryings) -> VolumeFragment {
    let eye = volume_mesh.eye.xyz;
    let offset = input.model_position - eye;
    let exit = length(offset);
    let direction = offset / max(exit, 1e-6);
    if (dot(input.normal, direction) < 0.0) {
        discard;
    }
    let hit = volume(eye, direction, volume_entry(eye, direction), exit, volume_mesh.footprint.x);
    let clip = volume_mesh.clip_from_model * vec4<f32>(hit.at, 1.0);
    // Nothing shows nearer than the camera's near plane.
    if (hit.color.a < 1.0 / 255.0 || clip.z < 0.0) {
        discard;
    }
    var output: VolumeFragment;
    output.color = hit.color * volume_mesh.footprint.y;
    output.depth = clip.z / clip.w;
    return output;
}
