use cranpose_ui_graphics::{TileMode, whole};

pub(crate) fn scissor_in_target(
    scissor: (f32, f32, f32, f32),
    target_size: (u32, u32),
    segment_offset: [f32; 2],
) -> Option<(u32, u32, u32, u32)> {
    let (x, y, width, height) = scissor;
    let left = whole::floor(x - segment_offset[0]).max(0.0);
    let top = whole::floor(y - segment_offset[1]).max(0.0);
    let right = whole::ceil(x + width - segment_offset[0]).min(target_size.0 as f32);
    let bottom = whole::ceil(y + height - segment_offset[1]).min(target_size.1 as f32);
    if right <= left || bottom <= top {
        return None;
    }
    Some((
        left as u32,
        top as u32,
        (right - left) as u32,
        (bottom - top) as u32,
    ))
}

pub(crate) fn intersect_scissors(
    a: Option<(u32, u32, u32, u32)>,
    b: Option<(u32, u32, u32, u32)>,
) -> Option<Option<(u32, u32, u32, u32)>> {
    match (a, b) {
        (None, None) => Some(None),
        (Some(rect), None) | (None, Some(rect)) => Some(Some(rect)),
        (Some((ax, ay, aw, ah)), Some((bx, by, bw, bh))) => {
            let left = ax.max(bx);
            let top = ay.max(by);
            let right = (ax + aw).min(bx + bw);
            let bottom = (ay + ah).min(by + bh);
            (right > left && bottom > top).then(|| Some((left, top, right - left, bottom - top)))
        }
    }
}

pub(crate) fn scaled_scissor(
    scissor: Option<(u32, u32, u32, u32)>,
    scale_x: f32,
    scale_y: f32,
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    let (x, y, w, h) = scissor?;
    if scale_x <= 1.0 && scale_y <= 1.0 {
        return scissor;
    }
    let left = (whole::floor(x as f32 / scale_x) as u32).min(width);
    let top = (whole::floor(y as f32 / scale_y) as u32).min(height);
    let right = (whole::ceil((x + w) as f32 / scale_x) as u32).min(width);
    let bottom = (whole::ceil((y + h) as f32 / scale_y) as u32).min(height);
    Some((
        left,
        top,
        right.saturating_sub(left),
        bottom.saturating_sub(top),
    ))
}

fn axis_span(start: i64, end: i64, margin: u32, len: u32, wraps: bool) -> (u32, u32) {
    let (low, high) = (start - i64::from(margin), end + i64::from(margin));
    if wraps && (low < 0 || high > i64::from(len)) {
        return (0, len.max(1));
    }
    let low = low.max(0) as u32;
    (low, (high.min(i64::from(len)) as u32).max(low + 1))
}

pub(crate) fn read_scissor(
    source: (u32, u32, u32, u32),
    scratch: (u32, u32, u32, u32),
    read: Option<(u32, u32, u32, u32)>,
    margin: (u32, u32),
    tile_mode: TileMode,
) -> (u32, u32, u32, u32) {
    let wraps = tile_mode == TileMode::Repeated;
    let Some((ux, uy, uw, uh)) = read else {
        return scratch;
    };
    let fx = scratch.2 as f32 / source.2.max(1) as f32;
    let fy = scratch.3 as f32 / source.3.max(1) as f32;
    let span = |start: u32, end: u32, origin: u32, factor: f32| {
        (
            whole::floor((start as f32 - origin as f32) * factor) as i64,
            whole::ceil((end as f32 - origin as f32) * factor) as i64,
        )
    };
    let (x0, x1) = span(ux, ux + uw, source.0, fx);
    let (y0, y1) = span(uy, uy + uh, source.1, fy);
    let (left, right) = axis_span(x0, x1, margin.0, scratch.2, wraps);
    let (top, bottom) = axis_span(y0, y1, margin.1, scratch.3, wraps);
    (
        scratch.0 + left,
        scratch.1 + top,
        right - left,
        bottom - top,
    )
}

pub(crate) fn widened_scissor(
    scissor: Option<(u32, u32, u32, u32)>,
    margin: (u32, u32),
    bounds: (u32, u32),
    tile_mode: TileMode,
) -> Option<(u32, u32, u32, u32)> {
    let wraps = tile_mode == TileMode::Repeated;
    let (x, y, width, height) = scissor?;
    let (left, right) = axis_span(
        i64::from(x),
        i64::from(x + width),
        margin.0,
        bounds.0,
        wraps,
    );
    let (top, bottom) = axis_span(
        i64::from(y),
        i64::from(y + height),
        margin.1,
        bounds.1,
        wraps,
    );
    Some((left, top, right - left, bottom - top))
}
