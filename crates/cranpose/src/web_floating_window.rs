pub(crate) fn outer_size_for_inner(
    requested: (f32, f32),
    outer: (f64, f64),
    inner: (f64, f64),
) -> (i32, i32) {
    let frame_width = (outer.0 - inner.0).max(0.0);
    let frame_height = (outer.1 - inner.1).max(0.0);
    (
        (f64::from(requested.0) + frame_width).round() as i32,
        (f64::from(requested.1) + frame_height).round() as i32,
    )
}

#[cfg(test)]
#[path = "tests/web_floating_window_tests.rs"]
mod tests;
