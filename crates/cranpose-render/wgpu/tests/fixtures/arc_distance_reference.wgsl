fn sdf_arc_band(
    p: vec2<f32>,
    center: vec2<f32>,
    inner: f32,
    outer: f32,
    mid_sin_cos: vec2<f32>,
    half_sin_cos: vec2<f32>,
    cap: u32,
) -> f32 {
    let ra = (outer + inner) * 0.5;
    let rb = max((outer - inner) * 0.5, 0.0);

    let sm = mid_sin_cos.x;
    let cm = mid_sin_cos.y;
    let d = p - center;
    var q = vec2<f32>(-sm * d.x + cm * d.y, cm * d.x + sm * d.y);
    q.x = abs(q.x);

    let sc = half_sin_cos;

    var dist: f32;
    if (sc.y * q.x > sc.x * q.y) {
        dist = length(q - sc * ra) - rb;
    } else {
        dist = abs(length(q) - ra) - rb;
    }

    let plane = sc.y * q.x - sc.x * q.y;
    if (cap == STROKE_CAP_BUTT) {
        dist = max(dist, plane);
    } else if (cap == STROKE_CAP_SQUARE) {
        dist = max(dist, plane - rb);
    }
    return dist;
}
