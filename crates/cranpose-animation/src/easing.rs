/// Easing functions for animations matching Jetpack Compose.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Easing {
    /// Linear interpolation (no easing).
    /// Jetpack Compose: LinearEasing
    LinearEasing,
    /// Ease in using cubic curve.
    /// Jetpack Compose: EaseIn (not a standard constant, but supported)
    EaseIn,
    /// Ease out using cubic curve.
    /// Jetpack Compose: EaseOut (not a standard constant, but supported)
    EaseOut,
    /// Ease in and out using cubic curve.
    /// Jetpack Compose: EaseInOut (not a standard constant, but supported)
    EaseInOut,
    /// Fast out, slow in (material design standard).
    /// Jetpack Compose: FastOutSlowInEasing
    FastOutSlowInEasing,
    /// Linear out, slow in (material design).
    /// Jetpack Compose: LinearOutSlowInEasing
    LinearOutSlowInEasing,
    /// Fast out, linear in (material design).
    /// Jetpack Compose: FastOutLinearEasing
    FastOutLinearEasing,
    /// A cubic Bezier timing curve with control points `(x1, y1)` and `(x2, y2)`.
    /// The x coordinates must be finite and in `[0, 1]` for a monotonic time mapping.
    CubicBezier(f32, f32, f32, f32),
    /// Holds each of `step_count` evenly spaced values for one discrete step.
    Steps(u32),
}

impl Easing {
    /// Apply the easing function to a linear fraction [0, 1].
    pub fn transform(&self, fraction: f32) -> f32 {
        match self {
            Self::LinearEasing => fraction,
            Self::EaseIn => cubic_bezier(0.42, 0.0, 1.0, 1.0, fraction),
            Self::EaseOut => cubic_bezier(0.0, 0.0, 0.58, 1.0, fraction),
            Self::EaseInOut => cubic_bezier(0.42, 0.0, 0.58, 1.0, fraction),
            Self::FastOutSlowInEasing => cubic_bezier(0.4, 0.0, 0.2, 1.0, fraction),
            Self::LinearOutSlowInEasing => cubic_bezier(0.0, 0.0, 0.2, 1.0, fraction),
            Self::FastOutLinearEasing => cubic_bezier(0.4, 0.0, 1.0, 1.0, fraction),
            Self::CubicBezier(x1, y1, x2, y2) => cubic_bezier(*x1, *y1, *x2, *y2, fraction),
            Self::Steps(step_count) => {
                if fraction <= 0.0 || fraction >= 1.0 {
                    fraction
                } else {
                    let step_count = (*step_count).max(1) as f32;
                    (fraction * step_count).floor() / step_count
                }
            }
        }
    }
}

/// Cubic bezier curve approximation for easing.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, fraction: f32) -> f32 {
    if fraction <= 0.0 {
        return 0.0;
    }
    if fraction >= 1.0 {
        return 1.0;
    }

    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;

    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;

    fn sample_curve(a: f32, b: f32, c: f32, t: f32) -> f32 {
        ((a * t + b) * t + c) * t
    }

    fn sample_derivative(a: f32, b: f32, c: f32, t: f32) -> f32 {
        (3.0 * a * t + 2.0 * b) * t + c
    }

    let mut t = fraction;
    let mut newton_success = false;
    for _ in 0..8 {
        let x = sample_curve(ax, bx, cx, t) - fraction;
        if x.abs() < 1e-6 {
            newton_success = true;
            break;
        }
        let dx = sample_derivative(ax, bx, cx, t);
        if dx.abs() < 1e-6 {
            break;
        }
        t = (t - x / dx).clamp(0.0, 1.0);
    }

    if !newton_success {
        let mut t0 = 0.0;
        let mut t1 = 1.0;
        t = fraction;
        for _ in 0..16 {
            let x = sample_curve(ax, bx, cx, t);
            let delta = x - fraction;
            if delta.abs() < 1e-6 {
                break;
            }
            if delta > 0.0 {
                t1 = t;
            } else {
                t0 = t;
            }
            t = 0.5 * (t0 + t1);
        }
    }

    sample_curve(ay, by, cy, t)
}
