// Rubber-band overscroll offset and invalidation callbacks for scroll containers.

use std::{cell::Cell, rc::Rc};

use super::{InvalidationCallbacks, app_owned_invalidation_callback};

const RUBBER_BAND_COEFFICIENT: f32 = 0.55;

fn rubber_band(raw: f32, dimension: f32) -> f32 {
    if !dimension.is_finite() || dimension <= 0.0 {
        return raw;
    }
    let x = raw.abs();
    let c = RUBBER_BAND_COEFFICIENT;
    (x * dimension * c / (dimension + c * x)).copysign(raw)
}

fn rubber_band_inverse(visible: f32, dimension: f32) -> f32 {
    if !dimension.is_finite() || dimension <= 0.0 {
        return visible;
    }
    let c = RUBBER_BAND_COEFFICIENT;
    let v = visible.abs().min(dimension * 0.999_9);
    (v * dimension / (c * (dimension - v))).copysign(visible)
}

#[derive(Clone)]
pub(crate) struct OverscrollEffect {
    inner: Rc<OverscrollEffectInner>,
}

struct OverscrollEffectInner {
    raw: Cell<f32>,
    visible: Cell<f32>,
    dimension: Cell<f32>,
    invalidate_callbacks: InvalidationCallbacks,
}

impl OverscrollEffect {
    pub(crate) fn new() -> Self {
        Self {
            inner: Rc::new(OverscrollEffectInner {
                raw: Cell::new(0.0),
                visible: Cell::new(0.0),
                dimension: Cell::new(0.0),
                invalidate_callbacks: InvalidationCallbacks::new(),
            }),
        }
    }

    pub(crate) fn offset(&self) -> f32 {
        self.inner.visible.get()
    }

    pub(crate) fn set_dimension(&self, dimension: f32) {
        if !dimension.is_finite() || dimension <= 0.0 {
            return;
        }
        self.inner.dimension.set(dimension);
        self.set_visible(rubber_band(self.inner.raw.get(), dimension));
    }

    pub(crate) fn apply_drag_delta(&self, delta: f32) -> f32 {
        if !delta.is_finite() || delta.abs() <= f32::EPSILON {
            return 0.0;
        }
        let before = self.offset();
        let raw = self.inner.raw.get() + delta;
        self.inner.raw.set(raw);
        self.set_visible(rubber_band(raw, self.inner.dimension.get()));
        self.offset() - before
    }

    pub(crate) fn apply_settle_delta(&self, delta: f32) -> f32 {
        let offset = self.offset();
        if offset.abs() <= f32::EPSILON {
            return 0.0;
        }
        let proposed = offset + delta;
        let crosses_edge = proposed.signum() != offset.signum();
        let next = if crosses_edge { 0.0 } else { proposed };
        let applied = next - offset;
        self.inner
            .raw
            .set(rubber_band_inverse(next, self.inner.dimension.get()));
        self.set_visible(next);
        applied
    }

    pub(crate) fn apply_to_scroll<F>(&self, delta: f32, perform_scroll: F) -> f32
    where
        F: FnOnce(f32) -> f32,
    {
        if !delta.is_finite() || delta.abs() <= f32::EPSILON {
            return 0.0;
        }
        let mut remaining = delta;
        let mut consumed = 0.0;
        let offset = self.offset();
        if offset.abs() > f32::EPSILON && delta.signum() != offset.signum() {
            let release = delta.abs().min(offset.abs()) * delta.signum();
            let released = self.apply_settle_delta(release);
            consumed += released;
            remaining -= released;
        }
        if remaining.abs() > f32::EPSILON {
            let target_consumed = perform_scroll(remaining);
            consumed += target_consumed;
            let unconsumed = remaining - target_consumed;
            if unconsumed.abs() > f32::EPSILON {
                consumed += self.apply_drag_delta(unconsumed);
            }
        }
        consumed
    }

    pub(crate) fn apply_to_fling<F>(&self, delta: f32, perform_scroll: F) -> f32
    where
        F: FnOnce(f32) -> f32,
    {
        let consumed = perform_scroll(delta);
        let unconsumed = delta - consumed;
        if unconsumed.abs() > f32::EPSILON {
            self.apply_drag_delta(-unconsumed);
        }
        consumed
    }

    pub(crate) fn add_invalidate_callback(&self, callback: Box<dyn Fn()>) -> u64 {
        self.inner
            .invalidate_callbacks
            .add(app_owned_invalidation_callback(callback))
    }

    pub(crate) fn remove_invalidate_callback(&self, id: u64) {
        self.inner.invalidate_callbacks.remove(id);
    }

    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    fn set_visible(&self, visible: f32) {
        if (visible - self.offset()).abs() <= f32::EPSILON {
            return;
        }
        self.inner.visible.set(visible);
        let callbacks = self.inner.invalidate_callbacks.snapshot();
        for callback in callbacks.values() {
            callback();
        }
    }
}
