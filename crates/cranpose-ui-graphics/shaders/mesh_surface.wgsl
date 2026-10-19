// Draws a composable's surface on a mesh in a 3D scene, with a depth test
// against the scene's other surfaces, and the depth of its shadow casters
// as the scene's light sees them. Glass reads the scene behind it from a
// copy the pass takes after the surfaces that are not glass. A portal's
// space draws inside its window, which the stencil marks, beyond the
// window's plane.
// It follows `mesh_common.wgsl`.

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

// The page behind the scene's glass, as the surfaces that are not glass
// left it.
@group(3) @binding(0) var backdrop: texture_2d<f32>;
@group(3) @binding(1) var backdrop_sampler: sampler;

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

// How bright the surface is where the light reaches it by `visibility`, from
// 0 in full shadow to 1. The side the viewer sees takes the light that falls
// on that side.
fn shade(input: VertexOutput, visibility: f32) -> f32 {
    if ((mesh.light.w < 0.0) | (mesh.params.y > 1.5)) {
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

// A surface in a portal's space, which shows only beyond its window's
// plane.
@fragment
fn mesh_portal_surface_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    if (before_window(input.scene_position)) {
        discard;
    }
    let color = content_color(input.uv);
    if (color.a < 1.0 / 255.0) {
        discard;
    }
    return shaded(color, shade(input, 1.0));
}

struct FarDepth {
    @builtin(frag_depth) depth: f32,
}

// A portal's window at the far plane, so its space draws over the surface
// the window lies on.
@fragment
fn mesh_window_far_fs() -> FarDepth {
    var output: FarDepth;
    output.depth = 1.0;
    return output;
}

@fragment
fn mesh_surface_shadowed_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let color = content_color(input.uv);
    if (color.a < 1.0 / 255.0) {
        discard;
    }
    // A surface that emits its colour takes no shadow, so reads no map.
    var visibility = 1.0;
    if (mesh.params.y < 1.5) {
        visibility = shadow_visibility(input.light_position);
    }
    return shaded(color, shade(input, visibility));
}

// The page's colour behind the glass at `pixel`, from the backdrop copy.
fn backdrop_at(pixel: vec2<f32>) -> vec3<f32> {
    let region = mesh.backdrop_region;
    let texel = clamp(pixel - region.xy, vec2<f32>(0.5), region.zw - vec2<f32>(0.5));
    let size = vec2<f32>(textureDimensions(backdrop));
    return textureSampleLevel(backdrop, backdrop_sampler, texel / size, 0.0).rgb;
}

// Where on the page the scene shows that the ray from the eye, seen at
// `pixel` on the glass at `position`, meets behind the glass. `eta` (one
// over the index) bends the ray toward the inside along `normal`, which
// faces the eye; the ray travels the glass's thickness inside, then bends as
// far again leaving it, as through a solid ball, and travels as far again.
fn bent_pixel(pixel: vec2<f32>, position: vec3<f32>, normal: vec3<f32>, eta: f32) -> vec2<f32> {
    let view = normalize(position - mesh.eye.xyz);
    let inside = refract(view, normal, eta);
    let leaving = normalize(2.0 * inside - view);
    let behind = position + (inside + leaving) * mesh.glass.y;
    let at = mesh.clip_from_scene * vec4<f32>(position, 1.0);
    let seen = mesh.clip_from_scene * vec4<f32>(behind, 1.0);
    if (seen.w <= 0.0) {
        return pixel;
    }
    let shift = seen.xy / seen.w - at.xy / at.w;
    return pixel + vec2<f32>(shift.x, -shift.y) * 0.5 * mesh.glass.zw;
}

@fragment
fn mesh_glass_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    return glass_color(input);
}

// Glass in a portal's space, beyond its window's plane.
@fragment
fn mesh_portal_glass_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    if (before_window(input.scene_position)) {
        discard;
    }
    return glass_color(input);
}

// The glass's colour: the page behind it bent through it, its reflection
// and its content over them.
fn glass_color(input: VertexOutput) -> vec4<f32> {
    let toward_eye = normalize(mesh.eye.xyz - input.scene_position);
    var normal = normalize(input.normal);
    if (dot(normal, toward_eye) < 0.0) {
        normal = -normal;
    }
    let pixel = input.clip_position.xy;
    let eta = mesh.glass.x;
    // Red bends a little less than blue, which splits colours at the rim.
    let through = vec3<f32>(
        backdrop_at(bent_pixel(pixel, input.scene_position, normal, eta * 1.012)).r,
        backdrop_at(bent_pixel(pixel, input.scene_position, normal, eta)).g,
        backdrop_at(bent_pixel(pixel, input.scene_position, normal, eta * 0.988)).b,
    ) * mesh.glass_tint.rgb;
    // Schlick's reflectance: the glass mirrors more of the scene toward its rim.
    let facing = clamp(dot(normal, toward_eye), 0.0, 1.0);
    let f0 = pow((1.0 - eta) / (1.0 + eta), 2.0);
    let reflectance = f0 + (1.0 - f0) * pow(1.0 - facing, 5.0);
    let mirror = reflect(-toward_eye, normal);
    var reflected = vec3<f32>(0.5 - 0.5 * mirror.y) * reflectance;
    if (mesh.light.w >= 0.0) {
        reflected += vec3<f32>(pow(max(0.0, -dot(mirror, mesh.light.xyz)), 96.0) * 2.0);
    }
    let glass = through * (1.0 - reflectance) + reflected;
    let content = content_color(input.uv);
    return vec4<f32>(content.rgb + glass * (1.0 - content.a), 1.0) * mesh.params.x;
}

// Writes the depth of a surface that is not glass, for the pass that draws
// glass after the copy it reads.
@fragment
fn mesh_depth_fs(input: VertexOutput) {
    if (content_color(input.uv).a < 1.0 / 255.0) {
        discard;
    }
}

// The depth of a surface in a portal's space that is not glass, beyond its
// window's plane.
@fragment
fn mesh_portal_depth_fs(input: VertexOutput) {
    if (before_window(input.scene_position) | (content_color(input.uv).a < 1.0 / 255.0)) {
        discard;
    }
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
