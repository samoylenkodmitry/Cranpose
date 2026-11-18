#version 450
// Writes a decoded picture's RGB. The immutable sampler of the pipeline holds the YCbCr
// conversion of the picture's hardware buffer, so the sampler returns the encoded R'G'B' values.

layout(set = 0, binding = 0) uniform sampler2D picture;

// The picture's part of the buffer in texture coordinates. `place` holds the coordinate of the
// part's first pixel and the size of one pixel. `inside` holds the lowest and the highest
// coordinate that the shader samples: the filter of the chroma samples reaches the neighbors of a
// coordinate, so the highest one stays a pixel inside the part, where the padding of the buffer
// does not enter.
layout(push_constant) uniform Window {
    vec4 place;
    vec4 inside;
} window;

layout(location = 0) out vec4 color;

void main() {
    vec2 at = window.place.xy + gl_FragCoord.xy * window.place.zw;
    color = vec4(texture(picture, clamp(at, window.inside.xy, window.inside.zw)).rgb, 1.0);
}
