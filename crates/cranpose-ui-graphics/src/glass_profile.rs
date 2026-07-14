//! Configurable physical cross-sections for liquid-glass surfaces.

/// Maximum knots in one principal-axis profile.
pub const MAX_GLASS_PROFILE_KNOTS: usize = 6;

const POSITION_EPSILON: f32 = 1.0e-5;

/// One normalized `(axis, height)` sample and its generated cubic tangent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassProfileKnot {
    position: f32,
    height: f32,
    tangent: f32,
}

impl GlassProfileKnot {
    const ZERO: Self = Self {
        position: 0.0,
        height: 0.0,
        tangent: 0.0,
    };

    pub fn position(self) -> f32 {
        self.position
    }

    pub fn height(self) -> f32 {
        self.height
    }

    pub fn tangent(self) -> f32 {
        self.tangent
    }
}

/// Validation failure while constructing a physical glass profile.
#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
pub enum GlassProfileError {
    #[error("a glass profile needs at least two knots")]
    TooFewKnots,
    #[error("a glass profile supports at most {MAX_GLASS_PROFILE_KNOTS} knots, got {count}")]
    TooManyKnots { count: usize },
    #[error("glass profile knot {index} contains a non-finite coordinate")]
    NonFiniteKnot { index: usize },
    #[error("glass profile knot {index} must stay in normalized 0..1 space")]
    KnotOutOfRange { index: usize },
    #[error("glass profile positions must start at 0 and end at 1")]
    MissingEndpoints,
    #[error("glass profile positions must increase strictly at knot {index}")]
    PositionsNotIncreasing { index: usize },
    #[error("X-Z and Y-Z profiles must share their center height")]
    CenterHeightMismatch,
    #[error("glass profile depth must be finite and non-negative")]
    InvalidDepth,
    #[error("glass profile radial power must be finite and in 1.5..8")]
    InvalidRadialPower,
    #[error("glass profile axis coupling must be finite and in 0..1")]
    InvalidAxisCoupling,
}

/// A normalized center-to-edge cubic cross-section.
///
/// Positions and heights are both authored in `0..1`. Tangents are generated
/// with a monotonicity-preserving cubic law so interactive sliders cannot
/// introduce unrequested oscillations between knots.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassProfileCurve {
    knots: [GlassProfileKnot; MAX_GLASS_PROFILE_KNOTS],
    len: u8,
}

impl GlassProfileCurve {
    pub fn from_points(points: &[(f32, f32)]) -> Result<Self, GlassProfileError> {
        if points.len() < 2 {
            return Err(GlassProfileError::TooFewKnots);
        }
        if points.len() > MAX_GLASS_PROFILE_KNOTS {
            return Err(GlassProfileError::TooManyKnots {
                count: points.len(),
            });
        }

        let mut knots = [GlassProfileKnot::ZERO; MAX_GLASS_PROFILE_KNOTS];
        for (index, &(position, height)) in points.iter().enumerate() {
            if !position.is_finite() || !height.is_finite() {
                return Err(GlassProfileError::NonFiniteKnot { index });
            }
            if !(0.0..=1.0).contains(&position) || !(0.0..=1.0).contains(&height) {
                return Err(GlassProfileError::KnotOutOfRange { index });
            }
            if index > 0 && position <= points[index - 1].0 {
                return Err(GlassProfileError::PositionsNotIncreasing { index });
            }
            knots[index] = GlassProfileKnot {
                position,
                height,
                tangent: 0.0,
            };
        }
        if points[0].0.abs() > POSITION_EPSILON
            || (points[points.len() - 1].0 - 1.0).abs() > POSITION_EPSILON
        {
            return Err(GlassProfileError::MissingEndpoints);
        }

        generate_monotone_tangents(&mut knots[..points.len()]);
        Ok(Self {
            knots,
            len: points.len() as u8,
        })
    }

    pub fn knots(&self) -> &[GlassProfileKnot] {
        &self.knots[..self.len as usize]
    }

    /// Evaluates `(height, d_height/d_position)` at a normalized coordinate.
    pub fn evaluate(&self, position: f32) -> (f32, f32) {
        let knots = self.knots();
        let position = position.clamp(0.0, 1.0);
        if position <= knots[0].position {
            return (knots[0].height, knots[0].tangent);
        }
        let last = knots.len() - 1;
        if position >= knots[last].position {
            return (knots[last].height, knots[last].tangent);
        }
        let segment = knots
            .windows(2)
            .position(|pair| position <= pair[1].position)
            .unwrap_or(last - 1);
        evaluate_hermite(knots[segment], knots[segment + 1], position)
    }

    fn constant(height: f32) -> Self {
        Self::from_points(&[(0.0, height), (1.0, height)])
            .expect("constant normalized profile is valid")
    }
}

impl Default for GlassProfileCurve {
    fn default() -> Self {
        Self::constant(0.5)
    }
}

/// Two principal cross-sections defining one coupled oval surface inside a
/// superelliptic aperture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassSurfaceProfile {
    x_profile: GlassProfileCurve,
    y_profile: GlassProfileCurve,
    depth: f32,
    radial_power: f32,
    axis_coupling: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlassSurfaceSample {
    pub height: f32,
    pub gradient: (f32, f32),
    pub radial_position: f32,
}

impl GlassSurfaceProfile {
    pub fn new(
        x_profile: GlassProfileCurve,
        y_profile: GlassProfileCurve,
        depth: f32,
        radial_power: f32,
    ) -> Result<Self, GlassProfileError> {
        if !depth.is_finite() || depth < 0.0 {
            return Err(GlassProfileError::InvalidDepth);
        }
        if !radial_power.is_finite() || !(1.5..=8.0).contains(&radial_power) {
            return Err(GlassProfileError::InvalidRadialPower);
        }
        if (x_profile.knots()[0].height - y_profile.knots()[0].height).abs() > POSITION_EPSILON {
            return Err(GlassProfileError::CenterHeightMismatch);
        }
        Ok(Self {
            x_profile,
            y_profile,
            depth,
            radial_power,
            axis_coupling: 0.0,
        })
    }

    pub fn isotropic(
        profile: GlassProfileCurve,
        depth: f32,
        radial_power: f32,
    ) -> Result<Self, GlassProfileError> {
        Self::new(profile, profile, depth, radial_power)
    }

    pub fn flat() -> Self {
        Self::isotropic(GlassProfileCurve::default(), 0.0, 2.0)
            .expect("flat surface profile is valid")
    }

    pub fn regular() -> Self {
        let curve = GlassProfileCurve::from_points(&[
            (0.0, 0.10),
            (0.50, 0.10),
            (0.70, 0.28),
            (0.86, 1.00),
            (1.0, 0.48),
        ])
        .expect("regular surface profile is valid");
        Self::isotropic(curve, 4.0, 3.6).expect("regular surface profile is coherent")
    }

    pub fn lens() -> Self {
        let profile = GlassProfileCurve::from_points(&[
            (0.0, 0.05),
            (0.16, 0.094),
            (0.30, 0.28),
            (0.52, 0.55),
            (0.76, 1.00),
            (1.0, 0.45),
        ])
        .expect("lens profile is valid");
        Self::isotropic(profile, 5.5, 3.2).expect("lens surface profile is coherent")
    }

    pub fn x_profile(self) -> GlassProfileCurve {
        self.x_profile
    }

    pub fn y_profile(self) -> GlassProfileCurve {
        self.y_profile
    }

    pub fn depth(self) -> f32 {
        self.depth
    }

    pub fn radial_power(self) -> f32 {
        self.radial_power
    }

    pub fn axis_coupling(self) -> f32 {
        self.axis_coupling
    }

    pub fn sample_normalized(self, position: (f32, f32)) -> GlassSurfaceSample {
        let x = position.0.clamp(-1.0, 1.0);
        let y = position.1.clamp(-1.0, 1.0);
        let abs_x = x.abs();
        let abs_y = y.abs();
        let a = abs_x.powf(self.radial_power);
        let b = abs_y.powf(self.radial_power);
        let q = a + b;
        if q <= 1.0e-6 {
            return GlassSurfaceSample {
                height: self.x_profile.evaluate(0.0).0,
                gradient: (0.0, 0.0),
                radial_position: 0.0,
            };
        }
        let radial = q.powf(1.0 / self.radial_power);
        let profile_position = radial.clamp(0.0, 1.0);
        let x_sample = self.x_profile.evaluate(profile_position);
        let y_sample = self.y_profile.evaluate(profile_position);
        let y_weight = b / q;
        let sign_x = if x < 0.0 { -1.0 } else { 1.0 };
        let sign_y = if y < 0.0 { -1.0 } else { 1.0 };
        let x_power = abs_x.powf(self.radial_power - 1.0) * sign_x;
        let y_power = abs_y.powf(self.radial_power - 1.0) * sign_y;
        let radial_factor = q.powf(1.0 / self.radial_power - 1.0);
        let radial_derivative = x_sample.1 + (y_sample.1 - x_sample.1) * y_weight;
        let profile_delta = y_sample.0 - x_sample.0;
        let q_squared = q * q;
        let oval_height = x_sample.0 + (y_sample.0 - x_sample.0) * y_weight;
        let oval_gradient = (
            radial_derivative * radial_factor * x_power
                - profile_delta * b * self.radial_power * x_power / q_squared,
            radial_derivative * radial_factor * y_power
                + profile_delta * a * self.radial_power * y_power / q_squared,
        );
        let x_axis_sample = self.x_profile.evaluate(abs_x);
        let y_axis_sample = self.y_profile.evaluate(abs_y);
        let center_height = self.x_profile.evaluate(0.0).0;
        let toric_height = x_axis_sample.0 + y_axis_sample.0 - center_height;
        let toric_gradient = (x_axis_sample.1 * sign_x, y_axis_sample.1 * sign_y);
        let coupling = self.axis_coupling;
        let height_delta = toric_height - oval_height;
        GlassSurfaceSample {
            height: oval_height + height_delta * coupling,
            gradient: (
                oval_gradient.0 + (toric_gradient.0 - oval_gradient.0) * coupling,
                oval_gradient.1 + (toric_gradient.1 - oval_gradient.1) * coupling,
            ),
            radial_position: profile_position,
        }
    }

    pub fn with_depth(self, depth: f32) -> Result<Self, GlassProfileError> {
        let mut profile = Self::new(self.x_profile, self.y_profile, depth, self.radial_power)?;
        profile.axis_coupling = self.axis_coupling;
        Ok(profile)
    }

    pub fn with_radial_power(self, radial_power: f32) -> Result<Self, GlassProfileError> {
        let mut profile = Self::new(self.x_profile, self.y_profile, self.depth, radial_power)?;
        profile.axis_coupling = self.axis_coupling;
        Ok(profile)
    }

    pub fn with_axis_coupling(mut self, axis_coupling: f32) -> Result<Self, GlassProfileError> {
        if !axis_coupling.is_finite() || !(0.0..=1.0).contains(&axis_coupling) {
            return Err(GlassProfileError::InvalidAxisCoupling);
        }
        self.axis_coupling = axis_coupling;
        Ok(self)
    }
}

impl Default for GlassSurfaceProfile {
    fn default() -> Self {
        Self::regular()
    }
}

fn generate_monotone_tangents(knots: &mut [GlassProfileKnot]) {
    let segment_count = knots.len() - 1;
    let mut widths = [0.0; MAX_GLASS_PROFILE_KNOTS - 1];
    let mut slopes = [0.0; MAX_GLASS_PROFILE_KNOTS - 1];
    for index in 0..segment_count {
        widths[index] = knots[index + 1].position - knots[index].position;
        slopes[index] = (knots[index + 1].height - knots[index].height) / widths[index];
    }

    // A center-to-edge radial profile is mirrored through the origin, so its
    // center tangent must be zero. The outer endpoint remains free to meet
    // the surrounding plane with the authored return slope.
    knots[0].tangent = 0.0;
    knots[segment_count].tangent = slopes[segment_count - 1];
    for index in 1..segment_count {
        let before = slopes[index - 1];
        let after = slopes[index];
        knots[index].tangent = if before * after <= 0.0 {
            0.0
        } else {
            let before_width = widths[index - 1];
            let after_width = widths[index];
            let first_weight = 2.0 * after_width + before_width;
            let second_weight = after_width + 2.0 * before_width;
            (first_weight + second_weight) / (first_weight / before + second_weight / after)
        };
    }
}

fn evaluate_hermite(start: GlassProfileKnot, end: GlassProfileKnot, position: f32) -> (f32, f32) {
    let width = end.position - start.position;
    let t = ((position - start.position) / width).clamp(0.0, 1.0);
    let t2 = t * t;
    let t3 = t2 * t;
    let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
    let h10 = t3 - 2.0 * t2 + t;
    let h01 = -2.0 * t3 + 3.0 * t2;
    let h11 = t3 - t2;
    let height = h00 * start.height
        + h10 * width * start.tangent
        + h01 * end.height
        + h11 * width * end.tangent;

    let dh00 = 6.0 * t2 - 6.0 * t;
    let dh10 = 3.0 * t2 - 4.0 * t + 1.0;
    let dh01 = -dh00;
    let dh11 = 3.0 * t2 - 2.0 * t;
    let tangent = (dh00 * start.height
        + dh10 * width * start.tangent
        + dh01 * end.height
        + dh11 * width * end.tangent)
        / width;
    (height, tangent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_validates_normalized_strictly_ordered_points() {
        assert_eq!(
            GlassProfileCurve::from_points(&[(0.0, 0.0)]),
            Err(GlassProfileError::TooFewKnots)
        );
        assert_eq!(
            GlassProfileCurve::from_points(&[(0.0, 0.0), (0.5, 0.5), (0.5, 1.0)]),
            Err(GlassProfileError::PositionsNotIncreasing { index: 2 })
        );
        assert_eq!(
            GlassProfileCurve::from_points(&[(0.1, 0.0), (1.0, 1.0)]),
            Err(GlassProfileError::MissingEndpoints)
        );
        assert_eq!(
            GlassProfileCurve::from_points(&[(0.0, -0.1), (1.0, 1.0)]),
            Err(GlassProfileError::KnotOutOfRange { index: 0 })
        );
        assert_eq!(
            GlassProfileCurve::from_points(&[(0.0, 0.0), (1.0, f32::NAN)]),
            Err(GlassProfileError::NonFiniteKnot { index: 1 })
        );
    }

    #[test]
    fn curve_evaluation_is_continuous_and_does_not_overshoot_knots() {
        let curve =
            GlassProfileCurve::from_points(&[(0.0, 0.1), (0.4, 0.1), (0.75, 1.0), (1.0, 0.4)])
                .expect("profile");
        assert_eq!(curve.evaluate(0.0), (0.1, 0.0));
        assert!((curve.evaluate(1.0).0 - 0.4).abs() < 1.0e-6);
        for step in 0..=100 {
            let height = curve.evaluate(step as f32 / 100.0).0;
            assert!((0.1 - 1.0e-5..=1.0 + 1.0e-5).contains(&height));
        }
        let left = curve.evaluate(0.75 - 1.0e-4).1;
        let right = curve.evaluate(0.75 + 1.0e-4).1;
        assert!((left - right).abs() < 0.02, "cubic tangent must stay C1");
    }

    #[test]
    fn surface_requires_a_coherent_center_and_physical_parameters() {
        let low = GlassProfileCurve::from_points(&[(0.0, 0.1), (1.0, 1.0)]).expect("low profile");
        let high = GlassProfileCurve::from_points(&[(0.0, 0.2), (1.0, 1.0)]).expect("high profile");
        assert_eq!(
            GlassSurfaceProfile::new(low, high, 4.0, 2.0),
            Err(GlassProfileError::CenterHeightMismatch)
        );
        assert_eq!(
            GlassSurfaceProfile::isotropic(low, -1.0, 2.0),
            Err(GlassProfileError::InvalidDepth)
        );
        assert_eq!(
            GlassSurfaceProfile::isotropic(low, 1.0, 8.1),
            Err(GlassProfileError::InvalidRadialPower)
        );
    }

    #[test]
    fn public_profile_accessors_report_the_authored_surface() {
        let profile = GlassSurfaceProfile::lens();
        assert_eq!(profile.x_profile().knots().len(), 6);
        assert_eq!(profile.y_profile().knots().len(), 6);
        assert!((profile.depth() - 5.5).abs() < 1.0e-6);
        assert!((profile.radial_power() - 3.2).abs() < 1.0e-6);
        let knot = profile.x_profile().knots()[3];
        assert!((knot.position() - 0.52).abs() < 1.0e-6);
        assert!((knot.height() - 0.55).abs() < 1.0e-6);
        assert!(knot.tangent().is_finite());

        let crest = profile.x_profile().knots()[4];
        assert!((crest.position() - 0.76).abs() < 1.0e-6);
        assert!((profile.x_profile().knots()[5].height() - 0.45).abs() < 1.0e-6);
    }

    #[test]
    fn lens_preset_is_a_recessed_face_with_one_raised_returning_meniscus() {
        let profile = GlassSurfaceProfile::lens();
        for curve in [profile.x_profile(), profile.y_profile()] {
            assert!(curve.evaluate(0.0).0 <= 0.15);
            assert!(curve.evaluate(0.50).1 >= 1.2);
            assert!((1.2..=2.5).contains(&curve.evaluate(0.65).1));
            assert!((-3.2..=-2.3).contains(&curve.evaluate(0.96).1));
            assert!((0.40..=0.50).contains(&curve.evaluate(1.0).0));
        }
    }

    #[test]
    fn flat_and_regular_presets_are_valid() {
        let flat = GlassSurfaceProfile::flat();
        assert_eq!(flat.depth(), 0.0);
        for profile in [GlassSurfaceProfile::regular(), GlassSurfaceProfile::lens()] {
            for curve in [profile.x_profile(), profile.y_profile()] {
                for knot in curve.knots() {
                    assert!((0.0..=1.0).contains(&knot.position()));
                    assert!((0.0..=1.0).contains(&knot.height()));
                    assert!(knot.tangent().is_finite());
                }
            }
        }
    }

    #[test]
    fn surface_builders_revalidate_physical_parameters() {
        let profile = GlassSurfaceProfile::lens()
            .with_depth(5.5)
            .expect("depth")
            .with_radial_power(4.0)
            .expect("power");
        assert_eq!(profile.depth(), 5.5);
        assert_eq!(profile.radial_power(), 4.0);
        assert_eq!(
            profile.with_depth(f32::NAN),
            Err(GlassProfileError::InvalidDepth)
        );
        assert_eq!(
            profile.with_radial_power(1.0),
            Err(GlassProfileError::InvalidRadialPower)
        );
        assert_eq!(
            profile.with_axis_coupling(-0.1),
            Err(GlassProfileError::InvalidAxisCoupling)
        );
        assert_eq!(
            profile.with_axis_coupling(1.1),
            Err(GlassProfileError::InvalidAxisCoupling)
        );
    }

    #[test]
    fn elliptical_surface_preserves_axis_profiles_and_cross_axis_curvature() {
        let x = GlassProfileCurve::from_points(&[(0.0, 0.1), (0.5, 0.3), (1.0, 0.8)])
            .expect("X-Z profile");
        let y = GlassProfileCurve::from_points(&[(0.0, 0.1), (0.5, 0.6), (1.0, 0.9)])
            .expect("Y-Z profile");
        let profile = GlassSurfaceProfile::new(x, y, 4.0, 2.0).expect("surface profile");

        let on_x = profile.sample_normalized((0.7, 0.0));
        let on_y = profile.sample_normalized((0.0, -0.4));
        let off_axis = profile.sample_normalized((0.7, -0.4));
        assert!((on_x.height - x.evaluate(0.7).0).abs() < 1.0e-6);
        assert!((on_y.height - y.evaluate(0.4).0).abs() < 1.0e-6);
        assert!(off_axis.gradient.0.abs() > 0.1);
        assert!(off_axis.gradient.1.abs() > 0.1);

        let flat_sided = GlassSurfaceProfile::new(x, y, 4.0, 6.0)
            .expect("flat-sided surface")
            .sample_normalized((0.7, -0.4));
        assert!(off_axis.gradient.1.abs() > flat_sided.gradient.1.abs());
    }

    #[test]
    fn axis_coupling_interpolates_sag_and_gradient_without_changing_axis_profiles() {
        let x = GlassProfileCurve::from_points(&[(0.0, 0.1), (0.5, 0.3), (1.0, 0.8)])
            .expect("X-Z profile");
        let y = GlassProfileCurve::from_points(&[(0.0, 0.1), (0.5, 0.6), (1.0, 0.9)])
            .expect("Y-Z profile");
        let oval = GlassSurfaceProfile::new(x, y, 4.0, 2.0).expect("oval");
        let toric = oval.with_axis_coupling(1.0).expect("toric surface");
        let halfway = oval.with_axis_coupling(0.5).expect("coupled surface");
        let position = (0.7, -0.4);
        let oval_sample = oval.sample_normalized(position);
        let halfway_sample = halfway.sample_normalized(position);

        assert_eq!(oval.axis_coupling(), 0.0);
        assert_eq!(toric.axis_coupling(), 1.0);
        assert!((toric.sample_normalized((0.7, 0.0)).height - x.evaluate(0.7).0).abs() < 1.0e-6);
        assert!((toric.sample_normalized((0.0, -0.4)).height - y.evaluate(0.4).0).abs() < 1.0e-6);
        let toric_height = x.evaluate(position.0).0 + y.evaluate(-position.1).0 - x.evaluate(0.0).0;
        assert!(
            (halfway_sample.height
                - (oval_sample.height + (toric_height - oval_sample.height) * 0.5))
                .abs()
                < 1.0e-6
        );
        let toric_sample = toric.sample_normalized(position);
        assert!((toric_sample.gradient.0 - x.evaluate(position.0).1).abs() < 1.0e-6);
        assert!((toric_sample.gradient.1 + y.evaluate(-position.1).1).abs() < 1.0e-6);

        let epsilon = 1.0e-4;
        let dx = (halfway
            .sample_normalized((position.0 + epsilon, position.1))
            .height
            - halfway
                .sample_normalized((position.0 - epsilon, position.1))
                .height)
            / (2.0 * epsilon);
        let dy = (halfway
            .sample_normalized((position.0, position.1 + epsilon))
            .height
            - halfway
                .sample_normalized((position.0, position.1 - epsilon))
                .height)
            / (2.0 * epsilon);
        assert!((halfway_sample.gradient.0 - dx).abs() < 2.0e-3);
        assert!((halfway_sample.gradient.1 - dy).abs() < 2.0e-3);
    }
}
