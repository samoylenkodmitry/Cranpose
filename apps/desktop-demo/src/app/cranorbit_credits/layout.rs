/// A dp position moved onto the nearest whole device pixel.
///
/// Kotlin's `roundToInt` sends an exact half up; Rust's `round` sends it away
/// from zero. They agree on everything a layout produces above the top of the
/// screen, and this keeps them agreeing below it too.
pub fn snap_to_pixel(value: f32, density: f32) -> f32 {
    if !(density > 0.0) || !value.is_finite() {
        return value;
    }
    (value * density + 0.5).floor() / density
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stack {
    cursor: f32,
    gap: f32,
    placed: bool,
}

impl Stack {
    pub fn total(gap: f32, heights: impl IntoIterator<Item = f32>) -> f32 {
        let mut total = 0.0;
        let mut count = 0usize;
        for height in heights {
            total += height;
            count += 1;
        }
        if count > 1 {
            total += gap * (count - 1) as f32;
        }
        total
    }

    /// Start a stack whose children sit centred on `centre_y`.
    ///
    /// Compose's `Arrangement.Center` measures every child in whole pixels and
    /// rounds the leading gap to a whole pixel before placing any of them, so a
    /// column whose content is an odd number of pixels tall starts on a pixel
    /// boundary rather than across one. This does the same, because half a
    /// pixel of drift is enough to move a line of text's antialiasing onto the
    /// next row — which is exactly what a screenshot comparison sees.
    pub fn centred(centre_y: f32, total: f32, gap: f32, density: f32) -> Self {
        Self {
            cursor: snap_to_pixel(centre_y - total * 0.5, density),
            gap,
            placed: false,
        }
    }

    pub fn from_top(top: f32, gap: f32) -> Self {
        Self {
            cursor: top,
            gap,
            placed: false,
        }
    }

    pub fn place(&mut self, height: f32) -> f32 {
        if self.placed {
            self.cursor += self.gap;
        }
        self.placed = true;
        let top = self.cursor;
        self.cursor += height;
        top
    }

    pub fn cursor(&self) -> f32 {
        self.cursor
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    pub top: f32,
    pub height: f32,
}

impl Slot {
    pub fn centre(self) -> f32 {
        self.top + self.height * 0.5
    }

    pub fn contains(self, y: f32) -> bool {
        y >= self.top && y < self.top + self.height
    }
}

pub fn slot_at(slots: &[Slot], y: f32) -> Option<usize> {
    for (index, slot) in slots.iter().enumerate() {
        let end = slots
            .get(index + 1)
            .map(|next| next.top)
            .unwrap_or(slot.top + slot.height);
        if y >= slot.top && y < end {
            return Some(index);
        }
    }
    None
}

pub fn scroll_by_pixels(slots: &[Slot], scroll: f32, delta: f32) -> f32 {
    if slots.len() < 2 {
        return scroll;
    }
    let last = slots.len() - 1;
    let mut scroll = scroll.clamp(0.0, last as f32);
    let mut remaining = delta;
    while remaining > 0.0 {
        let index = scroll.floor() as usize;
        if index >= last {
            return last as f32;
        }
        let step = slots[index + 1].centre() - slots[index].centre();
        if step <= 0.0 {
            return last as f32;
        }
        let available = step * ((index + 1) as f32 - scroll);
        if remaining < available {
            return scroll + remaining / step;
        }
        remaining -= available;
        scroll = (index + 1) as f32;
    }
    while remaining < 0.0 {
        if scroll <= 0.0 {
            return 0.0;
        }
        let index = (scroll.ceil() as usize).saturating_sub(1).min(last - 1);
        let step = slots[index + 1].centre() - slots[index].centre();
        if step <= 0.0 {
            return 0.0;
        }
        let available = step * (scroll - index as f32);
        if -remaining < available {
            return scroll + remaining / step;
        }
        remaining += available;
        scroll = index as f32;
    }
    scroll
}

pub fn slot_pitch(slots: &[Slot]) -> f32 {
    if slots.len() < 2 {
        return 0.0;
    }
    let first = slots[0].top;
    let last = slots[slots.len() - 1].top;
    (last - first) / (slots.len() - 1) as f32
}
