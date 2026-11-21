// A picture that the projection layer draws in place of a layer of its own: a flat rectangle or a
// part of a cylinder in the room, sampled from a texture.

struct Eye {
    // Takes a point of the room, in meters, to the clip space of the eye's image
    clip_from_room: mat4x4<f32>,
}

struct Layer {
    // The picture's center, and in w the radius of its cylinder, or 0 for a flat picture
    @location(0) center: vec4<f32>,
    // The picture's right axis, and in w its width in meters: its arc length on a cylinder
    @location(1) right: vec4<f32>,
    // The picture's up axis, and in w its height in meters
    @location(2) up: vec4<f32>,
    // The axis toward the viewer that the picture faces
    @location(3) toward: vec4<f32>,
    // The picture's part of its texture: left, top, right and bottom
    @location(4) part: vec4<f32>,
}

struct Varyings {
    @builtin(position) position: vec4<f32>,
    @location(0) at: vec2<f32>,
}

@group(0) @binding(0) var<uniform> eye: Eye;
@group(1) @binding(0) var picture: texture_2d<f32>;
@group(1) @binding(1) var picture_sampler: sampler;

// The strips across the picture. On a cylinder of a meter's radius, a strip of a panel 1.2 m wide
// leaves the curve by 0.2 mm at its middle.
const STRIPS: u32 = 32u;

// Two triangles for each strip, from the left edge to the right one.
@vertex
fn vs(@builtin(vertex_index) index: u32, layer: Layer) -> Varyings {
    let corner = index % 6u;
    // The corners 1, 4 and 5 lie on the strip's right side, and 2, 3 and 5 on its bottom
    let across = f32(index / 6u + ((50u >> corner) & 1u)) / f32(STRIPS);
    let down = f32((44u >> corner) & 1u);
    let radius = layer.center.w;
    let x = (across - 0.5) * layer.right.w;
    var sideways = x;
    var toward = 0.0;
    if (radius > 0.0) {
        // The cylinder's axis stands `radius` in front of the picture's center
        let angle = x / radius;
        sideways = radius * sin(angle);
        toward = radius * (1.0 - cos(angle));
    }
    let room = layer.center.xyz + layer.right.xyz * sideways
        + layer.up.xyz * ((0.5 - down) * layer.up.w) + layer.toward.xyz * toward;
    var out: Varyings;
    out.position = eye.clip_from_room * vec4<f32>(room, 1.0);
    out.at = vec2<f32>(mix(layer.part.x, layer.part.z, across), mix(layer.part.y, layer.part.w, down));
    return out;
}

// The picture's premultiplied color. A pixel that the picture leaves clear writes no depth, so the
// layers over the projection layer show through it.
@fragment
fn fs(in: Varyings) -> @location(0) vec4<f32> {
    let color = textureSample(picture, picture_sampler, in.at);
    if (color.a < 0.5 / 255.0) {
        discard;
    }
    return color;
}
