//! A row of tabs as a robot finds it: the axis it runs along and the span
//! of its tabs.

/// The direction along which a row of tabs is laid out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabAxis {
    /// Tabs side by side.
    Horizontal,
    /// Tabs stacked.
    Vertical,
}

// A rectangle's left, top, width and height in logical pixels.
pub(crate) type RectBounds = (f32, f32, f32, f32);
// A rectangle and the label of what it bounds.
pub(crate) type LabeledRect = (String, RectBounds);

/// The edges (left, top, right, bottom) of the rectangle that holds every
/// rectangle in `bounds`. `None` for an empty slice.
pub fn bounds_span(bounds: &[LabeledRect]) -> Option<RectBounds> {
    let mut iter = bounds.iter();
    let (_, (x, y, w, h)) = iter.next()?;
    let mut min_x = *x;
    let mut min_y = *y;
    let mut max_x = x + w;
    let mut max_y = y + h;
    for (_, (bx, by, bw, bh)) in iter {
        min_x = min_x.min(*bx);
        min_y = min_y.min(*by);
        max_x = max_x.max(*bx + *bw);
        max_y = max_y.max(*by + *bh);
    }
    Some((min_x, min_y, max_x, max_y))
}

/// The axis along which `bounds` spread the most.
pub fn detect_tab_axis(bounds: &[LabeledRect]) -> Option<TabAxis> {
    let (min_x, min_y, max_x, max_y) = bounds_span(bounds)?;
    let span_x = max_x - min_x;
    let span_y = max_y - min_y;
    if span_x >= span_y {
        Some(TabAxis::Horizontal)
    } else {
        Some(TabAxis::Vertical)
    }
}
