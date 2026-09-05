use crate::{ArcGeometry, TAU};

#[derive(Clone, Debug, Default)]
struct AngleTrig {
    last: Option<(u32, (f32, f32))>,
}

impl AngleTrig {
    fn resolve(&mut self, angle: f32) -> (f32, f32) {
        let key = angle.to_bits();
        if let Some((held, result)) = self.last
            && held == key
        {
            return result;
        }
        let result = angle.sin_cos();
        self.last = Some((key, result));
        result
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ArcTrigCache {
    mid: AngleTrig,
    half: AngleTrig,
}

impl ArcTrigCache {
    pub(crate) fn resolve(&mut self, geometry: &ArcGeometry) -> [f32; 4] {
        if geometry.sweep_angle >= TAU && geometry.start_angle == 0.0 {
            return [0.0, -1.0, 0.0, -1.0];
        }
        let half_sweep = geometry.sweep_angle.clamp(0.0, TAU) * 0.5;
        let (mid_sin, mid_cos) = self.mid.resolve(geometry.start_angle + half_sweep);
        let (half_sin, half_cos) = self.half.resolve(half_sweep);
        [mid_sin, mid_cos, half_sin.max(0.0), half_cos]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Point, StrokeCap};

    #[test]
    fn reuse_preserves_bits_when_either_angle_changes() {
        let mut cache = ArcTrigCache::default();
        for (start, sweep) in [
            (0.3, 0.1),
            (0.3, 0.1),
            (0.4, 0.1),
            (0.4, 0.2),
            (0.3, 0.1),
            (0.0, TAU),
            (0.4, 0.2),
            (0.0, -0.0),
            (-0.0, -0.0),
            (f32::INFINITY, 0.1),
            (0.3, f32::NAN),
            (0.3, 0.1),
        ] {
            let geometry = ArcGeometry {
                center: Point::ZERO,
                inner_radius: 10.0,
                outer_radius: 12.0,
                start_angle: start,
                sweep_angle: sweep,
                cap: StrokeCap::Butt,
            };
            let expected = if sweep >= TAU && start == 0.0 {
                [0.0, -1.0, 0.0, -1.0]
            } else {
                let half = sweep.clamp(0.0, TAU) * 0.5;
                let (mid_sin, mid_cos) = (start + half).sin_cos();
                let (half_sin, half_cos) = half.sin_cos();
                [mid_sin, mid_cos, half_sin.max(0.0), half_cos]
            };
            assert_eq!(
                cache.resolve(&geometry).map(f32::to_bits),
                expected.map(f32::to_bits)
            );
        }
    }
}
