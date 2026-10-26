use super::{current_app_context, require_current_app_context, with_render_state};

pub(super) fn normalize_density(density: f32) -> f32 {
    if density.is_finite() && density > 0.0 {
        density
    } else {
        1.0
    }
}

fn normalize_font_scale(scale: f32) -> f32 {
    if scale.is_finite() && scale > 0.0 {
        scale.clamp(MIN_FONT_SCALE, MAX_FONT_SCALE)
    } else {
        1.0
    }
}

/// Smallest system font scale honoured; below this, text stops being readable
/// as text.
pub const MIN_FONT_SCALE: f32 = 0.5;
/// Largest system font scale honoured. Android's own accessibility slider tops
/// out at 2.0.
pub const MAX_FONT_SCALE: f32 = 3.0;

/// Returns the current density scale factor (logical px per dp).
pub fn current_density() -> f32 {
    with_render_state(|state| f32::from_bits(state.density_bits.get()))
}

/// Updates the current density scale factor.
///
/// This triggers a global layout invalidation when the value changes because
/// density impacts layout, text measurement, and input thresholds.
pub fn set_density(density: f32) {
    let normalized = normalize_density(density);
    let new_bits = normalized.to_bits();
    with_render_state(|state| {
        let old_bits = state.density_bits.replace(new_bits);
        if old_bits != new_bits {
            state.layout_invalidated.set(true);
        }
    });
}

/// Returns the system font scale — the multiplier the user chose in the
/// platform's font-size setting, `1.0` when they left it alone.
///
/// This is the setting `Sp` is defined against, so text follows it while
/// everything measured in `Dp` does not. A platform that does not report one
/// leaves it at `1.0`.
///
/// It is the number to *report*, not the number to multiply by: what a size in
/// `Sp` comes to is [`scale_sp`], and on Android 14 and up the two are not the
/// same arithmetic. See [`crate::font_scale`].
pub fn current_font_scale() -> f32 {
    current_font_scale_curve().scale()
}

/// Returns the conversion the platform performs for a size in `Sp`.
///
/// The setting is not a multiplier on every platform — see
/// [`crate::font_scale`] — so this, and not [`current_font_scale`], is what a
/// size in `Sp` is resolved through. The scalar remains the thing to *report*.
pub fn current_font_scale_curve() -> crate::font_scale::FontScaleCurve {
    with_render_state(|state| state.font_scale.get())
}

/// The current app context's density and font scale curve, read through one
/// lookup of the context, or `None` outside a context.
pub(crate) fn current_host_density() -> Option<(f32, crate::font_scale::FontScaleCurve)> {
    current_app_context().map(|context| {
        (
            f32::from_bits(context.state.density_bits.get()),
            context.state.font_scale.get(),
        )
    })
}

/// The running app's font scale curve, or `None` when it leaves every size as
/// it is, which the caller then need not read.
pub(crate) fn current_scaling_font_scale_curve() -> Option<crate::font_scale::FontScaleCurve> {
    with_render_state(|state| (!state.font_scale_identity.get()).then(|| state.font_scale.get()))
}

/// What a prepared text layout is valid for: the text service's generation
/// and the font scale curve's fingerprint, read in one app context access.
pub(crate) fn text_layout_stamp() -> (u64, u32) {
    let context = require_current_app_context("text layout stamp access");
    (
        context.text.generation(),
        context.state.font_scale_fingerprint.get(),
    )
}

/// A size in scale-independent pixels, in dp, through the running app's curve.
pub fn scale_sp(sp: f32) -> f32 {
    current_font_scale_curve().sp_to_dp(sp)
}

/// Updates the system font scale, taking it as a plain multiplier.
///
/// Hosts call this when the platform reports the setting, and again whenever it
/// changes while the app is running — on Android that is a configuration
/// change, which arrives without the process restarting. Like density it
/// invalidates layout, because every `Sp` size on screen has just changed.
///
/// A host whose platform converts `Sp` through a table of its own calls
/// [`set_font_scale_curve`] instead.
pub fn set_font_scale(scale: f32) {
    set_font_scale_curve(crate::font_scale::FontScaleCurve::linear(scale));
}

/// Updates the system font scale and the conversion behind it.
///
/// Hosts that can read the platform's real `Sp` conversion call this instead of
/// [`set_font_scale`], which is the same thing with no table behind it. A curve
/// whose scale is not a value a platform could report is refused the same way a
/// bare scalar is, and refusing it drops the table with it: knots sampled at a
/// scale that was rejected describe a conversion the app is not going to use.
pub fn set_font_scale_curve(curve: crate::font_scale::FontScaleCurve) {
    let normalized = normalize_font_scale(curve.scale());
    let curve = if (normalized - curve.scale()).abs() <= f32::EPSILON {
        curve
    } else {
        crate::font_scale::FontScaleCurve::linear(normalized)
    };
    with_render_state(|state| {
        if state.font_scale.get() != curve {
            state.font_scale.set(curve);
            state.font_scale_identity.set(curve.is_identity());
            state.font_scale_fingerprint.set(curve.fingerprint());
            state.layout_invalidated.set(true);
        }
    });
}
