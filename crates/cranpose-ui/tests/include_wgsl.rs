// `include_wgsl!` embeds WGSL files joined in order, without their comments
// or the whitespace round each line, one line for each line of a file.

use cranpose_ui::include_wgsl;

const SURFACE_WGSL: &str = include_wgsl!("fixtures/wgsl/noise.wgsl", "fixtures/wgsl/surface.wgsl");

#[test]
fn files_join_in_order_without_comments_and_keep_their_lines() {
    assert_eq!(
        SURFACE_WGSL,
        "\n\
         fn noise(p: vec2<f32>) -> f32 {\n\
         return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);\n\
         }\n\
         \n\
         \n\
         fn shade(uv: vec2<f32>) -> f32 {\n\
         return noise(uv);\n\
         }\n",
        "the comment lines stay as empty lines, the trailing comment and the indents go"
    );
}
