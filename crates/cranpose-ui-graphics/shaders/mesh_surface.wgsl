// Draws a composable's surface on a mesh in a 3D scene, with a depth test
// against the scene's other surfaces.

struct MeshUniforms {
    clip_from_model: mat4x4<f32>,
    scene_from_model: mat4x4<f32>,
    // Rows of the inverse transpose of the model matrix's 3x3: turns model
    // normals into scene space.
    normal_x: vec4<f32>,
    normal_y: vec4<f32>,
    normal_z: vec4<f32>,
    // xyz = the eye in scene space.
    eye: vec4<f32>,
    // xyz = the direction the light travels, of length one; w = the ambient
    // share, or below zero for an unlit surface.
    light: vec4<f32>,
    // xy = the texel where the content starts; zw = its size in texels.
    source_region: vec4<f32>,
    // x = the surface's alpha.
    params: vec4<f32>,
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
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(1) @binding(0) var<uniform> mesh: MeshUniforms;

@vertex
fn mesh_surface_vs(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    output.clip_position = mesh.clip_from_model * vec4<f32>(input.position, 1.0);
    output.scene_position = (mesh.scene_from_model * vec4<f32>(input.position, 1.0)).xyz;
    output.uv = input.uv;
    output.normal = vec3<f32>(
        dot(mesh.normal_x.xyz, input.normal),
        dot(mesh.normal_y.xyz, input.normal),
        dot(mesh.normal_z.xyz, input.normal),
    );
    return output;
}

@fragment
fn mesh_surface_fs(input: VertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(input_texture));
    // Half a texel in from the content's edge, so a surface that shares its
    // texture never filters in a neighbour's texels.
    let first = mesh.source_region.xy + vec2<f32>(0.5);
    let last = mesh.source_region.xy + mesh.source_region.zw - vec2<f32>(0.5);
    let texel = clamp(mesh.source_region.xy + input.uv * mesh.source_region.zw, first, last);
    let color = textureSample(input_texture, input_sampler, texel / size);
    if (color.a < 1.0 / 255.0) {
        discard;
    }
    var shade = 1.0;
    if (mesh.light.w >= 0.0) {
        // The side of the surface the viewer sees takes the light that falls
        // on that side.
        let toward_eye = mesh.eye.xyz - input.scene_position;
        let seen = select(-input.normal, input.normal, dot(input.normal, toward_eye) >= 0.0);
        let normal_length = length(seen);
        let lit = select(0.0, max(0.0, -dot(seen, mesh.light.xyz)) / normal_length, normal_length > 0.0);
        shade = mesh.light.w + (1.0 - mesh.light.w) * lit;
    }
    return vec4<f32>(color.rgb * shade, color.a) * mesh.params.x;
}
