use cranpose::liquid::{Glass, LiquidModifierExt, LiquidShape};
use cranpose_ui::{Color, Modifier};

pub(super) const INK: Color = Color(0.91, 0.95, 0.98, 1.0);

fn glass() -> Glass {
    let mut glass = Glass::clear();
    glass.shape = LiquidShape::RoundedRect(0.0);
    glass.tint = Some(Color(0.045, 0.085, 0.12, 0.24));
    glass.blur_radius = Some(0.8);
    glass.dispersion = 1.0;
    glass.lift = Some(0.0);
    glass.contrast = Some(1.0);
    glass.saturation = Some(1.0);
    glass.refraction_depth_dp = Some(12.0);
    glass.transmission_refraction = 0.2;
    glass.foreground = Some(INK);
    glass.adaptive_frost = 0.0;
    glass.highlight = 0.25;
    glass.rim_reflection = 0.2;
    glass.face_lighting = false;
    glass.shadow = false;
    glass
}

pub(super) fn reader_surface(modifier: Modifier) -> Modifier {
    modifier.glass_effect(glass())
}

pub(super) fn code_surface(modifier: Modifier) -> Modifier {
    let mut glass = glass();
    glass.tint = Some(Color(0.012, 0.023, 0.036, 0.48));
    glass.blur_radius = Some(8.0);
    modifier.glass_effect(glass)
}
