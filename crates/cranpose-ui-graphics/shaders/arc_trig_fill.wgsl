const BODY_ROW_WORDS: u32 = 16u;
const BODY_FLAGS_WORD: u32 = 9u;
const CURVE_ROW_VECTORS: u32 = 2u;
const TRIG_FILL_WORKGROUP: u32 = 64u;

@group(0) @binding(1)
var<storage, read_write> fill_bodies: array<u32>;

@group(0) @binding(2)
var<storage, read_write> fill_curves: array<vec4<f32>>;

@compute @workgroup_size(TRIG_FILL_WORKGROUP)
fn cs_arc_trig(@builtin(global_invocation_id) id: vec3<u32>) {
    let row = id.x;
    if (row >= arrayLength(&fill_curves) / CURVE_ROW_VECTORS
        || row >= arrayLength(&fill_bodies) / BODY_ROW_WORDS) {
        return;
    }
    if ((fill_bodies[row * BODY_ROW_WORDS + BODY_FLAGS_WORD] & 3u) != RECORD_KIND_ARC) {
        return;
    }
    let angles = fill_curves[row * CURVE_ROW_VECTORS + 1u];
    fill_curves[row * CURVE_ROW_VECTORS] = arc_trig(angles.x, angles.y);
}
